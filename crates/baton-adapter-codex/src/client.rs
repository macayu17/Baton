use baton_protocol::*;
use baton_store::{Actor, Store};
use baton_supervisor::Session;
use serde::Deserialize;
use std::time::Duration;
use tokio::io::BufReader;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{ChildStdin, ChildStdout};

const FRAME_LIMIT: usize = 65_536;
#[path = "registration.rs"]
mod registration;
fn error(code: ErrorCode) -> Diagnostic {
    Diagnostic::new(code)
}

/// Vendor-reported platform metadata, shape-checked only; not a native attestation.
#[derive(Debug, PartialEq, Eq)]
pub struct Runtime {
    pub platform_family: String,
    pub platform_os: String,
}

/// A live initialized transport, not permission to start a turn or continue a task.
pub struct Source {
    session: Session,
    _reader: BufReader<ChildStdout>,
    _writer: ChildStdin,
    runtime: Runtime,
    allow_file_creation: bool,
    experimental: bool,
}
struct Connecting<'a> {
    session: Option<Session>,
    store: &'a mut Store,
    actor: &'a Actor,
    gap: bool,
}
impl Connecting<'_> {
    fn close(&mut self) -> Result<(), Diagnostic> {
        if let Some(mut session) = self.session.take() {
            let gap = if self.gap {
                (|| {
                    let binding = session.binding();
                    let run = self
                        .store
                        .run(self.actor, binding.task_id, binding.run_id)?;
                    if matches!(run.state, RunState::Running | RunState::Stopping) {
                        session.observe(
                            self.store,
                            self.actor,
                            Id::new_v4(),
                            RunObservation::AdapterGap {
                                source_sequence: run
                                    .source_cursor
                                    .checked_add(1)
                                    .ok_or_else(|| error(ErrorCode::Blocked))?,
                            },
                        )?;
                    }
                    Ok(())
                })()
            } else {
                Ok(())
            };
            let lost = session.connection_lost(self.store, self.actor);
            gap?;
            lost?;
        }
        Ok(())
    }
}
impl Drop for Connecting<'_> {
    fn drop(&mut self) {
        // Cancellation also terminates the owned scope; journal failure cannot authorize replay.
        let _ = self.close();
    }
}
impl Source {
    /// Validates initialize and flushes initialized; the notification has no peer acknowledgement.
    pub async fn connect(
        session: Session,
        store: &mut Store,
        actor: &Actor,
        timeout: Duration,
    ) -> Result<Self, Diagnostic> {
        Self::connect_policy(session, store, actor, timeout, false).await
    }
    /// Explicit experimental handshake for the fixed client-mediated file tool.
    pub async fn connect_with_file_tools(
        session: Session,
        store: &mut Store,
        actor: &Actor,
        timeout: Duration,
    ) -> Result<Self, Diagnostic> {
        Self::connect_policy(session, store, actor, timeout, true).await
    }
    async fn connect_policy(
        session: Session,
        store: &mut Store,
        actor: &Actor,
        timeout: Duration,
        experimental: bool,
    ) -> Result<Self, Diagnostic> {
        let mut connecting = Connecting {
            session: Some(session),
            store,
            actor,
            gap: false,
        };
        let attempt = async {
            if timeout.is_zero() || timeout > Duration::from_secs(30) {
                return Err(error(ErrorCode::InvalidInput));
            }
            let session = connecting
                .session
                .as_mut()
                .ok_or_else(|| error(ErrorCode::SupervisionFailure))?;
            let binding = session.binding();
            if binding.actor_id != actor.id() {
                return Err(error(ErrorCode::Forbidden));
            }
            let run = connecting
                .store
                .run(actor, binding.task_id, binding.run_id)?;
            if &run.binding != binding || run.state != RunState::Running || run.event_gap {
                return Err(error(ErrorCode::Blocked));
            }
            let (stdin, stdout) = session.take_stdio()?;
            let mut writer = ChildStdin::from_std(stdin).map_err(|_| error(ErrorCode::Io))?;
            let mut reader =
                BufReader::new(ChildStdout::from_std(stdout).map_err(|_| error(ErrorCode::Io))?);
            let runtime =
                tokio::time::timeout(timeout, initialize(&mut reader, &mut writer, experimental))
                    .await
                    .map_err(|_| error(ErrorCode::AdapterTimeout))??;
            if session.root_exited()? {
                return Err(error(ErrorCode::AdapterProtocol));
            }
            Ok((runtime, reader, writer))
        }
        .await;
        match attempt {
            Ok((runtime, reader, writer)) => Ok(Self {
                session: connecting
                    .session
                    .take()
                    .ok_or_else(|| error(ErrorCode::SupervisionFailure))?,
                _reader: reader,
                _writer: writer,
                runtime,
                allow_file_creation: experimental,
                experimental,
            }),
            Err(error) => {
                connecting.close()?;
                Err(error)
            }
        }
    }
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
    /// Explicit trusted-host policy; does not activate a source or register vendor tools.
    pub fn with_file_creation(mut self) -> Self {
        self.allow_file_creation = true;
        self
    }
    fn record_event(
        session: &mut Session,
        store: &mut Store,
        actor: &Actor,
        event: AdapterEvent,
    ) -> Result<RunRecord, Diagnostic> {
        let binding = session.binding().clone();
        let operation_id = crate::wire::operation_id(&binding, &event)?;
        let current = store.run(actor, binding.task_id, binding.run_id)?;
        let source_sequence = if let Some(cached) =
            store.execution_event(actor, binding.task_id, binding.run_id, operation_id)?
        {
            let RunObservation::AdapterReported {
                source_sequence, ..
            } = cached.observation
            else {
                return Err(error(ErrorCode::CorruptStore));
            };
            source_sequence
        } else {
            current
                .source_cursor
                .checked_add(1)
                .ok_or_else(|| error(ErrorCode::Blocked))?
        };
        session.observe(
            store,
            actor,
            operation_id,
            RunObservation::AdapterReported {
                source_sequence,
                event,
            },
        )
    }
    /// Capture one supported lifecycle fact. A retry result is history, not writer authority.
    pub async fn capture_next(
        self,
        store: &mut Store,
        actor: &Actor,
        adapter: &AgentMetadata,
        thread: &str,
        timeout: Duration,
    ) -> Result<(Self, RunRecord), Diagnostic> {
        let Source {
            session,
            _reader: mut reader,
            _writer: mut writer,
            runtime,
            allow_file_creation,
            experimental,
        } = self;
        let mut guard = Connecting {
            session: Some(session),
            store,
            actor,
            gap: true,
        };
        let attempt = async {
            if timeout.is_zero() || timeout > Duration::from_secs(30) {
                return Err(error(ErrorCode::InvalidInput));
            }
            let session = guard
                .session
                .as_mut()
                .ok_or_else(|| error(ErrorCode::SupervisionFailure))?;
            let binding = session.binding().clone();
            if binding.actor_id != actor.id() {
                return Err(error(ErrorCode::Forbidden));
            }
            if guard
                .store
                .run(actor, binding.task_id, binding.run_id)?
                .source_thread
                .as_ref()
                .is_some_and(|id| id != thread)
            {
                return Err(error(ErrorCode::Forbidden));
            }
            let deadline = tokio::time::Instant::now() + timeout;
            let bytes = tokio::time::timeout_at(deadline, frame(&mut reader))
                .await
                .map_err(|_| error(ErrorCode::AdapterTimeout))??;
            if registration::remote_disabled(&bytes)? {
                return registration::record_remote_disabled(session, guard.store, actor, &bytes);
            }
            if let Some(id) = registration::announcement(&bytes)? {
                if id != thread {
                    return Err(error(ErrorCode::AdapterProtocol));
                }
                return registration::record_announcement(session, guard.store, actor, &id, &bytes);
            }
            match crate::wire::incoming(&bytes, adapter, thread)? {
                crate::wire::Incoming::InterruptReply(request_id) => {
                    let operation = crate::wire::control_operation_id(
                        &binding,
                        adapter,
                        thread,
                        &request_id,
                        8,
                    )?;
                    let planned = guard
                        .store
                        .execution_event(actor, binding.task_id, binding.run_id, operation)?
                        .ok_or_else(|| error(ErrorCode::AdapterProtocol))?;
                    let RunObservation::AdapterReported { mut event, .. } = planned.observation
                    else {
                        return Err(error(ErrorCode::CorruptStore));
                    };
                    if event.adapter != *adapter
                        || event.thread_id != thread
                        || !matches!(&event.event,
                        AdapterEventKind::TurnInterruptPlanned{request_id:id} if *id==request_id)
                    {
                        return Err(error(ErrorCode::AdapterProtocol));
                    }
                    event.event = AdapterEventKind::TurnInterruptAcknowledged { request_id };
                    Self::record_event(session, guard.store, actor, event)
                }
                crate::wire::Incoming::Event => {
                    let event = crate::wire::normalize(&bytes, &binding, adapter, thread)?;
                    Self::record_event(session, guard.store, actor, event)
                }
                crate::wire::Incoming::Callback(event)
                    if allow_file_creation
                        && matches!(
                            event.event,
                            AdapterEventKind::CallbackRefusal {
                                callback: CallbackKind::DynamicTool,
                                ..
                            }
                        ) =>
                {
                    let recipe = crate::wire::file_creation(&bytes)?;
                    let AdapterEventKind::CallbackRefusal {
                        request_id,
                        correlation_id,
                        payload_hash,
                        ..
                    } = event.event
                    else {
                        return Err(error(ErrorCode::AdapterProtocol));
                    };
                    let planned_id = crate::wire::control_operation_id(
                        &binding,
                        adapter,
                        thread,
                        &request_id,
                        5,
                    )?;
                    if guard
                        .store
                        .execution_event(actor, binding.task_id, binding.run_id, planned_id)?
                        .is_some()
                    {
                        return Err(error(ErrorCode::IdempotencyConflict));
                    }
                    let mut planned = AdapterEvent {
                        event: AdapterEventKind::DynamicToolExecution(Box::new(
                            ToolExecutionCallback {
                                request_id: request_id.clone(),
                                correlation_id,
                                payload_hash,
                                phase: CallbackPhase::Planned,
                                result_hash: None,
                            },
                        )),
                        ..event
                    };
                    Self::record_event(session, guard.store, actor, planned.clone())?;
                    if tokio::time::Instant::now() >= deadline {
                        return Err(error(ErrorCode::AdapterTimeout));
                    }
                    let operation = crate::wire::control_operation_id(
                        &binding,
                        adapter,
                        thread,
                        &request_id,
                        12,
                    )?;
                    let digest = session.create_file(
                        guard.store,
                        actor,
                        operation,
                        &recipe.name,
                        recipe.content.as_bytes(),
                    )?;
                    if tokio::time::Instant::now() >= deadline {
                        return Err(error(ErrorCode::AdapterTimeout));
                    }
                    let reply = crate::wire::creation_reply(&request_id, &digest)?;
                    tokio::time::timeout_at(deadline, async {
                        writer.write_all(&reply).await?;
                        writer.flush().await
                    })
                    .await
                    .map_err(|_| error(ErrorCode::AdapterTimeout))?
                    .map_err(|_| error(ErrorCode::Io))?;
                    if let AdapterEventKind::DynamicToolExecution(callback) = &mut planned.event {
                        callback.phase = CallbackPhase::Written;
                        callback.result_hash = Some(digest);
                    }
                    Self::record_event(session, guard.store, actor, planned)
                }
                crate::wire::Incoming::Approval(event) | crate::wire::Incoming::Callback(event) => {
                    let mut written = event.clone();
                    let (request_id, reply) = match &event.event {
                        AdapterEventKind::ApprovalCancellationPlanned {
                            request_id,
                            item_id,
                            tool,
                        } => {
                            written.event = AdapterEventKind::ApprovalCancellationWritten {
                                request_id: request_id.clone(),
                                item_id: item_id.clone(),
                                tool: *tool,
                            };
                            (
                                request_id.clone(),
                                Some(crate::wire::cancel_reply(request_id)?),
                            )
                        }
                        AdapterEventKind::CallbackRefusal {
                            request_id,
                            callback,
                            phase: CallbackPhase::Planned,
                            ..
                        } => {
                            if let AdapterEventKind::CallbackRefusal { phase, .. } =
                                &mut written.event
                            {
                                *phase = CallbackPhase::Written;
                            }
                            (
                                request_id.clone(),
                                if *callback == CallbackKind::DynamicTool {
                                    Some(crate::wire::dynamic_failure(request_id)?)
                                } else {
                                    None
                                },
                            )
                        }
                        _ => return Err(error(ErrorCode::AdapterProtocol)),
                    };
                    let resolved = crate::wire::control_operation_id(
                        &binding,
                        adapter,
                        thread,
                        &request_id,
                        7,
                    )?;
                    if guard
                        .store
                        .execution_event(actor, binding.task_id, binding.run_id, resolved)?
                        .is_some()
                    {
                        return Err(error(ErrorCode::IdempotencyConflict));
                    }
                    Self::record_event(session, guard.store, actor, event)?;
                    let reply = reply.ok_or_else(|| error(ErrorCode::ExecutionNotEnabled))?;
                    if tokio::time::Instant::now() >= deadline {
                        return Err(error(ErrorCode::AdapterTimeout));
                    }
                    tokio::time::timeout_at(deadline, async {
                        writer.write_all(&reply).await?;
                        writer.flush().await
                    })
                    .await
                    .map_err(|_| error(ErrorCode::AdapterTimeout))?
                    .map_err(|_| error(ErrorCode::Io))?;
                    Self::record_event(session, guard.store, actor, written)
                }
                crate::wire::Incoming::Resolution(request_id) => {
                    let operation = crate::wire::control_operation_id(
                        &binding,
                        adapter,
                        thread,
                        &request_id,
                        5,
                    )?;
                    let planned = guard
                        .store
                        .execution_event(actor, binding.task_id, binding.run_id, operation)?
                        .ok_or_else(|| error(ErrorCode::AdapterProtocol))?;
                    let RunObservation::AdapterReported { mut event, .. } = planned.observation
                    else {
                        return Err(error(ErrorCode::CorruptStore));
                    };
                    if event.adapter != *adapter || event.thread_id != thread {
                        return Err(error(ErrorCode::AdapterProtocol));
                    }
                    if matches!(event.event, AdapterEventKind::DynamicToolExecution(_)) {
                        let written_id = crate::wire::control_operation_id(
                            &binding,
                            adapter,
                            thread,
                            &request_id,
                            6,
                        )?;
                        let written = guard
                            .store
                            .execution_event(actor, binding.task_id, binding.run_id, written_id)?
                            .ok_or_else(|| error(ErrorCode::AdapterProtocol))?;
                        let RunObservation::AdapterReported { event: saved, .. } =
                            written.observation
                        else {
                            return Err(error(ErrorCode::CorruptStore));
                        };
                        event = saved;
                    }
                    match &mut event.event {
                        AdapterEventKind::DynamicToolExecution(callback)
                            if callback.request_id == request_id
                                && callback.phase == CallbackPhase::Written =>
                        {
                            callback.phase = CallbackPhase::Resolved;
                        }
                        AdapterEventKind::ApprovalCancellationPlanned {
                            request_id: id, ..
                        } if *id == request_id => {
                            event.event =
                                AdapterEventKind::ApprovalResolutionReported { request_id }
                        }
                        AdapterEventKind::CallbackRefusal {
                            request_id: id,
                            callback: CallbackKind::DynamicTool,
                            phase,
                            ..
                        } if *id == request_id && *phase == CallbackPhase::Planned => {
                            *phase = CallbackPhase::Resolved
                        }
                        _ => return Err(error(ErrorCode::AdapterProtocol)),
                    }
                    Self::record_event(session, guard.store, actor, event)
                }
            }
        }
        .await;
        match attempt {
            Ok(record) => Ok((
                Self {
                    session: guard
                        .session
                        .take()
                        .ok_or_else(|| error(ErrorCode::SupervisionFailure))?,
                    _reader: reader,
                    _writer: writer,
                    runtime,
                    allow_file_creation,
                    experimental,
                },
                record,
            )),
            Err(error) => {
                guard.close()?;
                Err(error)
            }
        }
    }
    /// Request cancellation once for an observed active turn; acknowledgement is captured separately.
    pub async fn request_interrupt(
        self,
        store: &mut Store,
        actor: &Actor,
        adapter: &AgentMetadata,
        thread: &str,
        turn: &str,
        timeout: Duration,
    ) -> Result<(Self, RunRecord), Diagnostic> {
        let Source {
            session,
            _reader: reader,
            _writer: mut writer,
            runtime,
            allow_file_creation,
            experimental,
        } = self;
        let mut guard = Connecting {
            session: Some(session),
            store,
            actor,
            gap: true,
        };
        let attempt = async {
            if timeout.is_zero() || timeout > Duration::from_secs(30) {
                return Err(error(ErrorCode::InvalidInput));
            }
            let deadline = tokio::time::Instant::now() + timeout;
            let session = guard.session.as_mut()
                .ok_or_else(|| error(ErrorCode::SupervisionFailure))?;
            let binding = session.binding().clone();
            if binding.actor_id != actor.id() {
                return Err(error(ErrorCode::Forbidden));
            }
            let request_id = crate::wire::interrupt_id(&binding, adapter, thread, turn)?;
            let event = AdapterEvent {
                adapter: adapter.clone(),
                thread_id: thread.into(),
                turn_id: turn.into(),
                event: AdapterEventKind::TurnInterruptPlanned { request_id: request_id.clone() },
            };
            event.validate()?;
            let mut written = event.clone();
            written.event = AdapterEventKind::TurnInterruptWritten { request_id: request_id.clone() };
            let operation = crate::wire::operation_id(&binding, &event)?;
            let current = guard.store.run(actor, binding.task_id, binding.run_id)?;
            if !matches!(current.state, RunState::Running | RunState::Stopping) {
                return Err(error(ErrorCode::Blocked));
            }
            if let Some(prior) = guard.store.execution_event(actor, binding.task_id, binding.run_id, operation)? {
                if !matches!(prior.observation, RunObservation::AdapterReported { event: old, .. } if old == event) {
                    return Err(error(ErrorCode::IdempotencyConflict));
                }
                let key = crate::wire::operation_id(&binding, &written)?;
                let saved = guard.store.execution_event(actor, binding.task_id, binding.run_id, key)?.ok_or_else(||error(ErrorCode::Blocked))?;
                if !matches!(saved.observation, RunObservation::AdapterReported { event: old, .. } if old == written) {
                    return Err(error(ErrorCode::CorruptStore));
                }
                return Ok(current);
            }
            let bytes = crate::wire::interrupt_request(&request_id, thread, turn)?;
            Self::record_event(session, guard.store, actor, event)?;
            if tokio::time::Instant::now() >= deadline {
                return Err(error(ErrorCode::AdapterTimeout));
            }
            tokio::time::timeout_at(deadline, async {
                writer.write_all(&bytes).await?;
                writer.flush().await
            }).await.map_err(|_| error(ErrorCode::AdapterTimeout))?
                .map_err(|_| error(ErrorCode::Io))?;
            Self::record_event(session, guard.store, actor, written)
        }.await;
        match attempt {
            Ok(record) => Ok((
                Self {
                    session: guard
                        .session
                        .take()
                        .ok_or_else(|| error(ErrorCode::SupervisionFailure))?,
                    _reader: reader,
                    _writer: writer,
                    runtime,
                    allow_file_creation,
                    experimental,
                },
                record,
            )),
            Err(error) => {
                guard.close()?;
                Err(error)
            }
        }
    }
    pub fn binding(&self) -> &RunBinding {
        self.session.binding()
    }
    pub fn stop(
        mut self,
        store: &mut Store,
        actor: &Actor,
        timeout: Duration,
    ) -> Result<RunRecord, Diagnostic> {
        self.session.stop(store, actor, timeout)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VendorRuntime {
    user_agent: String,
    codex_home: String,
    platform_family: String,
    platform_os: String,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Reply {
    Success(InitializeSuccess),
    Failure(InitializeFailure),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitializeSuccess {
    id: u64,
    result: VendorRuntime,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitializeFailure {
    id: u64,
    #[serde(rename = "error")]
    _error: serde::de::IgnoredAny,
}

async fn frame(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<Vec<u8>, Diagnostic> {
    let mut bytes = Vec::new();
    loop {
        let pending = reader.fill_buf().await.map_err(|_| error(ErrorCode::Io))?;
        if pending.is_empty() {
            return Err(error(ErrorCode::AdapterProtocol));
        }
        let newline = pending.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(pending.len(), |pos| pos + 1);
        if bytes.len() + count > FRAME_LIMIT {
            return Err(error(ErrorCode::AdapterProtocol));
        }
        bytes.extend_from_slice(&pending[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(bytes);
        }
    }
}

async fn initialize<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    experimental: bool,
) -> Result<Runtime, Diagnostic> {
    let mut request =
        serde_json::to_vec(&serde_json::json!({"id":1,"method":"initialize","params":{
            "clientInfo":{"name":"baton","version":"0.1.0"},
            "capabilities":{"experimentalApi":experimental,"explicitGatewayOauth":true}
        }}))
        .map_err(|_| error(ErrorCode::InvalidInput))?;
    request.push(b'\n');
    writer
        .write_all(&request)
        .await
        .map_err(|_| error(ErrorCode::Io))?;
    writer.flush().await.map_err(|_| error(ErrorCode::Io))?;
    let reply: Reply = serde_json::from_slice(&frame(reader).await?)
        .map_err(|_| error(ErrorCode::AdapterProtocol))?;
    let runtime = match reply {
        Reply::Success(reply) if reply.id == 1 => reply.result,
        Reply::Failure(reply) if reply.id == 1 => return Err(error(ErrorCode::AdapterRejected)),
        _ => return Err(error(ErrorCode::AdapterProtocol)),
    };
    if runtime.user_agent.is_empty()
        || runtime.user_agent.len() > 1024
        || runtime.codex_home.is_empty()
        || runtime.codex_home.len() > 16_384
        || !matches!(runtime.platform_family.as_str(), "unix" | "windows")
        || !matches!(runtime.platform_os.as_str(), "linux" | "macos" | "windows")
        || (runtime.platform_family == "windows") != (runtime.platform_os == "windows")
    {
        return Err(error(ErrorCode::AdapterProtocol));
    }
    writer
        .write_all(b"{\"method\":\"initialized\",\"params\":{}}\n")
        .await
        .map_err(|_| error(ErrorCode::Io))?;
    writer.flush().await.map_err(|_| error(ErrorCode::Io))?;
    Ok(Runtime {
        platform_family: runtime.platform_family,
        platform_os: runtime.platform_os,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    // M5-C02; FR-15, INV-09: invalid input cannot trigger initialized or leak vendor text.
    #[tokio::test]
    async fn malformed_uncorrelated_or_oversized_replies_are_not_acknowledged() {
        let good = json!({"id":1,"result":{
            "userAgent":"synthetic", "codexHome":"/private/secret",
            "platformFamily":"unix","platformOs":"linux"
        }});
        let mut wrong_id = good.clone();
        wrong_id["id"] = json!(2);
        let mut missing = good.clone();
        missing["result"]
            .as_object_mut()
            .unwrap()
            .remove("codexHome");
        let mut both = good.clone();
        both["error"] = json!({"message":"private token"});
        let mut mixed_null = good.clone();
        mixed_null["error"] = json!(null);
        let mut unsupported = good.clone();
        unsupported["result"]["platformOs"] = json!("unsupported");
        let mut unknown = good.clone();
        unknown["authority"] = json!(true);
        for reply in [
            wrong_id.to_string(),
            missing.to_string(),
            both.to_string(),
            mixed_null.to_string(),
            "{\"id\":1}".into(),
            unsupported.to_string(),
            unknown.to_string(),
            "{\"id\":1,\"id\":1,\"result\":null}".into(),
            "{\"method\":\"item/started\",\"params\":{}}".into(),
            "not json".into(),
            "".into(),
            "x".repeat(FRAME_LIMIT),
        ] {
            let mut reader = std::io::Cursor::new(format!("{reply}\n").into_bytes());
            let mut writer = Vec::new();
            let error = initialize(&mut reader, &mut writer, false)
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::AdapterProtocol);
            assert!(!String::from_utf8(writer)
                .unwrap()
                .contains("\"method\":\"initialized\""));
            assert!(!error.to_string().contains("private"));
        }
        let mut writer = Vec::new();
        let error = initialize(
            &mut std::io::Cursor::new(
                b"{\"id\":1,\"error\":{\"code\":-1,\"message\":\"private token\"}}\n",
            ),
            &mut writer,
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::AdapterRejected);
        assert!(!error.to_string().contains("token"));
    }

    // M5-C02; partial messages and partial UTF-8 cannot be mistaken for complete replies.
    #[tokio::test]
    async fn truncated_frames_and_eof_fail_closed() {
        for bytes in [b"".as_slice(), b"{\"id\":1}", b"\xff\n"] {
            let error = initialize(&mut std::io::Cursor::new(bytes), &mut Vec::new(), false)
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::AdapterProtocol);
        }
    }

    // M5-C02; FR-03/15, INV-09: only the correlated initialize response permits initialized.
    #[tokio::test]
    async fn initialization_correlates_before_acknowledging_without_starting_work() {
        let (client, server) = tokio::io::duplex(8192);
        let (read, mut write) = tokio::io::split(client);
        let (server_read, mut server_write) = tokio::io::split(server);
        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(server_read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "initialize");
            assert_eq!(request["params"]["clientInfo"]["name"], "baton");
            assert_eq!(request["params"]["capabilities"]["experimentalApi"], false);
            let reply = json!({"id":request["id"],"result":{
                "userAgent":"synthetic/0.163.0", "codexHome":"/synthetic/private",
                "platformFamily":"unix","platformOs":"linux"
            }});
            server_write
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            let ack: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(ack, json!({"method":"initialized","params":{}}));
            line.clear();
            assert_eq!(reader.read_line(&mut line).await.unwrap(), 0);
        });
        let runtime = initialize(&mut BufReader::new(read), &mut write, false)
            .await
            .unwrap();
        assert_eq!(runtime.platform_os, "linux");
        assert!(!format!("{runtime:?}").contains("private"));
        drop(write);
        server.await.unwrap();
    }
}
