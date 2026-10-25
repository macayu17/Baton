use baton_platform::{ipc, LocalAuthority};
use baton_protocol::*;
use baton_store::{Actor, Store, SystemClock};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

fn error(code: ErrorCode) -> Diagnostic {
    Diagnostic::new(code)
}
pub fn response(request_id: Id, result: Result<LocalReply, Diagnostic>) -> LocalResponse {
    LocalResponse {
        schema_version: SchemaVersion::V0_1_0,
        request_id,
        continuation_quality: RecoveryQuality::Blocked,
        continuation_reasons: vec![ErrorCode::ExecutionNotEnabled],
        result,
    }
}

pub fn initialize(root: &Path, repo: &Path, request_id: Id) -> Result<LocalReply, Diagnostic> {
    let mut store = if root.try_exists().map_err(|_| error(ErrorCode::Io))? {
        Store::open(root, Arc::new(SystemClock))?
    } else {
        Store::create(root, Arc::new(SystemClock))?
    };
    let authority = LocalAuthority::acquire(root)?;
    let actor = store.recover_local_identity(&authority)?;
    let request = Request {
        schema_version: SchemaVersion::V0_1_0,
        operation_id: request_id,
        actor_id: actor.id(),
        expected_epoch: 0,
        action: Action::EnrollRepository {
            repository_id: request_id,
            root: repo
                .to_str()
                .ok_or_else(|| error(ErrorCode::InvalidInput))?
                .into(),
        },
    };
    store
        .execute_local(&actor, &request)
        .map(LocalReply::Mutation)
}

struct Coordinator {
    store: Store,
    actor: Actor,
}
impl Coordinator {
    fn dispatch(&mut self, request: LocalRequest) -> LocalResponse {
        let id = request.request_id;
        let result = if id.is_nil() || request.expected_epoch > i64::MAX as u64 {
            Err(error(ErrorCode::InvalidInput))
        } else {
            match request.command {
                LocalCommand::Invoke { action } => self
                    .store
                    .execute_local(
                        &self.actor,
                        &Request {
                            schema_version: request.schema_version,
                            operation_id: id,
                            actor_id: self.actor.id(),
                            expected_epoch: request.expected_epoch,
                            action,
                        },
                    )
                    .map(LocalReply::Mutation),
                LocalCommand::Status { task_id } => {
                    self.store.task(&self.actor, task_id).map(LocalReply::Task)
                }
                LocalCommand::Tasks { after, limit } => self
                    .store
                    .tasks_page(&self.actor, after, limit)
                    .map(LocalReply::Tasks),
                LocalCommand::Events {
                    task_id,
                    after,
                    limit,
                } => self
                    .store
                    .events_page(&self.actor, task_id, after, limit)
                    .map(LocalReply::Events),
                LocalCommand::Checkpoint {
                    task_id,
                    checkpoint_id,
                } => self
                    .store
                    .checkpoint(&self.actor, task_id, checkpoint_id)
                    .map(LocalReply::Checkpoint),
                LocalCommand::Run { task_id, run_id } => self
                    .store
                    .run(&self.actor, task_id, run_id)
                    .map(LocalReply::Run),
                LocalCommand::Transfer {
                    task_id,
                    transfer_id,
                } => self
                    .store
                    .transfer(&self.actor, task_id, transfer_id)
                    .map(LocalReply::Transfer),
            }
        };
        response(id, result)
    }
}

/// Explicit opt-in daemon; bounded OS-scoped IPC only, no process launch endpoint.
pub async fn serve(root: &Path, on_ready: impl FnOnce(Id)) -> Result<(), Diagnostic> {
    let authority = LocalAuthority::acquire(root)?;
    let mut store = Store::open(root, Arc::new(SystemClock))?;
    let actor = store.recover_local_identity(&authority)?;
    let session = Id::new_v4();
    let listener = ipc::bind(authority, session)?;
    store.publish_local_session(listener.authority(), session)?;
    let coordinator = Arc::new(Mutex::new(Coordinator { store, actor }));
    let connections = Arc::new(tokio::sync::Semaphore::new(16));
    on_ready(session);
    loop {
        tokio::select! {
            stopped = tokio::signal::ctrl_c() => { return stopped.map_err(|_| error(ErrorCode::Io)); }
            accepted = listener.accept() => {
                let Ok(mut stream) = accepted else { continue; };
                let Ok(permit) = connections.clone().try_acquire_owned() else { continue; };
                let coordinator = coordinator.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let raw = tokio::time::timeout(Duration::from_secs(5), ipc::read_frame(&mut stream, LOCAL_REQUEST_LIMIT)).await;
                    let Ok(Ok(raw)) = raw else { return; };
                    let result = match serde_json::from_slice::<LocalRequest>(&raw) {
                        Ok(request) => tokio::task::spawn_blocking(move || {
                            match coordinator.lock() {
                                Ok(mut coordinator) => coordinator.dispatch(request),
                                Err(_) => response(request.request_id, Err(error(ErrorCode::CorruptStore))),
                            }
                        }).await.unwrap_or_else(|_| response(Id::nil(), Err(error(ErrorCode::StoreFailure)))),
                        Err(_) => response(Id::nil(), Err(error(ErrorCode::InvalidInput))),
                    };
                    if let Ok(bytes) = serde_json::to_vec(&result) {
                        let _ = tokio::time::timeout(Duration::from_secs(10), ipc::write_frame(&mut stream, &bytes, LOCAL_RESPONSE_LIMIT)).await;
                    }
                });
            }
        }
    }
}

pub async fn request(root: PathBuf, request: LocalRequest) -> Result<LocalResponse, Diagnostic> {
    let session = Store::published_local_session(&root).map_err(|e| {
        if e.code == ErrorCode::NotFound {
            error(ErrorCode::DaemonUnavailable)
        } else {
            e
        }
    })?;
    let id = request.request_id;
    let bytes = serde_json::to_vec(&request).map_err(|_| error(ErrorCode::InvalidInput))?;
    let exchange = async {
        let mut stream = ipc::connect(&root, session).await.map_err(|e| {
            if e.code == ErrorCode::Io {
                error(ErrorCode::DaemonUnavailable)
            } else {
                e
            }
        })?;
        ipc::write_frame(&mut stream, &bytes, LOCAL_REQUEST_LIMIT)
            .await
            .map_err(|e| {
                if e.code == ErrorCode::Io {
                    error(ErrorCode::ResponseLost)
                } else {
                    e
                }
            })?;
        let raw = ipc::read_frame(&mut stream, LOCAL_RESPONSE_LIMIT)
            .await
            .map_err(|_| error(ErrorCode::ResponseLost))?;
        let result: LocalResponse =
            serde_json::from_slice(&raw).map_err(|_| error(ErrorCode::InvalidInput))?;
        if result.request_id != id {
            return Err(error(ErrorCode::InvalidInput));
        }
        Ok(result)
    };
    tokio::time::timeout(Duration::from_secs(120), exchange)
        .await
        .map_err(|_| error(ErrorCode::ResponseLost))?
}
