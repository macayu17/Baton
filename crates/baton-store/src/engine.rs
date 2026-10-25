//! Transactional implementation for the experimental in-process API.

use baton_protocol::*;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const APPLICATION_ID: i64 = 0x4241544e;
const FILE: &str = "baton.sqlite3";
mod execution;
mod identity;
mod native;
mod restores;
mod snapshots;
mod transfers;
pub use execution::{LaunchPermit, ToolPermit};

fn error(code: ErrorCode) -> Diagnostic {
    Diagnostic::new(code)
}
fn sql_error(e: rusqlite::Error) -> Diagnostic {
    use rusqlite::ErrorCode as SqlCode;
    error(match e.sqlite_error_code() {
        Some(SqlCode::DatabaseBusy | SqlCode::DatabaseLocked) => ErrorCode::StoreBusy,
        Some(SqlCode::DatabaseCorrupt | SqlCode::NotADatabase) => ErrorCode::CorruptStore,
        _ => ErrorCode::StoreFailure,
    })
}
trait Db<T> {
    fn db(self) -> Result<T, Diagnostic>;
}
impl<T> Db<T> for rusqlite::Result<T> {
    fn db(self) -> Result<T, Diagnostic> {
        self.map_err(sql_error)
    }
}
fn encode(value: &impl serde::Serialize) -> Result<String, Diagnostic> {
    serde_json::to_string(value).map_err(|_| error(ErrorCode::InvalidInput))
}
fn decode<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, Diagnostic> {
    serde_json::from_str(json).map_err(|_| error(ErrorCode::CorruptStore))
}

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> Result<i64, Diagnostic>;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> Result<i64, Diagnostic> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .ok_or_else(|| error(ErrorCode::Blocked))
    }
}

/// An in-process capability. Serialized actor IDs cannot construct this value.
/// OS/IPC authentication and secure credential persistence are not implemented.
#[derive(Clone)]
pub struct Actor {
    id: Id,
    secret: [u8; 32],
}
impl Actor {
    fn fresh(id: Id) -> Self {
        let mut secret = [0; 32];
        secret[..16].copy_from_slice(Id::new_v4().as_bytes());
        secret[16..].copy_from_slice(Id::new_v4().as_bytes());
        Self { id, secret }
    }
    pub fn id(&self) -> Id {
        self.id
    }
    fn hash(&self) -> String {
        blake3::hash(&self.secret).to_hex().to_string()
    }
}
impl fmt::Debug for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Actor")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

pub struct Store {
    connection: Connection,
    root: PathBuf,
    clock: Arc<dyn Clock>,
}

impl Store {
    /// Creates a new owned directory only; an existing directory is never adopted.
    pub fn create(root: &Path, clock: Arc<dyn Clock>) -> Result<Self, Diagnostic> {
        if !root.is_absolute() {
            return Err(error(ErrorCode::InvalidInput));
        }
        let parent = root
            .parent()
            .ok_or_else(|| error(ErrorCode::InvalidInput))?;
        baton_platform::validate_parent(parent)?;
        let parent = fs::canonicalize(parent).map_err(|_| error(ErrorCode::Io))?;
        ensure_outside_git(&parent)?;
        let root = parent.join(
            root.file_name()
                .ok_or_else(|| error(ErrorCode::InvalidInput))?,
        );
        baton_platform::create_private_dir(&root)?;
        baton_platform::private_file(&root.join(FILE))?;
        let mut connection = connect(&root)?;
        configure(&connection)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .db()?;
        tx.execute_batch(include_str!("schema.sql")).db()?;
        baton_checkpoint::Vault::create(&root.join("checkpoints"))?;
        tx.commit().db()?;
        Ok(Self {
            connection,
            root: fs::canonicalize(root).map_err(|_| error(ErrorCode::Io))?,
            clock,
        })
    }

    pub fn open(root: &Path, clock: Arc<dyn Clock>) -> Result<Self, Diagnostic> {
        ensure_outside_git(root)?;
        let connection = connect(root)?;
        let app: i64 = connection
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .db()?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .db()?;
        if app != APPLICATION_ID || version != 15 {
            return Err(error(ErrorCode::UnsupportedSchema));
        }
        configure(&connection)?;
        baton_checkpoint::Vault::open(&root.join("checkpoints"))?;
        let check: String = connection
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .db()?;
        if check != "ok"
            || connection
                .prepare("PRAGMA foreign_key_check")
                .db()?
                .exists([])
                .db()?
        {
            return Err(error(ErrorCode::CorruptStore));
        }
        Ok(Self {
            connection,
            root: fs::canonicalize(root).map_err(|_| error(ErrorCode::Io))?,
            clock,
        })
    }

    /// Trusted local bootstrap only. Never expose actor creation to an LLM client.
    pub fn create_local_actor(&mut self) -> Result<Actor, Diagnostic> {
        let actor = Actor::fresh(Id::new_v4());
        self.connection
            .execute(
                "INSERT INTO actors VALUES (?1, ?2)",
                params![actor.id.to_string(), actor.hash()],
            )
            .db()?;
        Ok(actor)
    }

    pub fn execute(&mut self, actor: &Actor, request: &Request) -> Result<Reply, Diagnostic> {
        self.reserve_restore(actor, request, false)?;
        self.execute_fresh(actor, request, false)
            .map(|(reply, _)| reply)
    }

    /// OS-authenticated local control, with no supervisor/actor authority operations.
    pub fn execute_local(&mut self, actor: &Actor, request: &Request) -> Result<Reply, Diagnostic> {
        self.reserve_restore(actor, request, true)?;
        self.execute_fresh(actor, request, true)
            .map(|(reply, _)| reply)
    }

    fn execute_fresh(
        &mut self,
        actor: &Actor,
        request: &Request,
        local: bool,
    ) -> Result<(Reply, bool), Diagnostic> {
        self.execute_context(actor, request, local, None, None)
    }

    fn execute_context(
        &mut self,
        actor: &Actor,
        request: &Request,
        local: bool,
        native: Option<native::Evidence<'_>>,
        activation: Option<&execution::ReceiverLaunch<'_>>,
    ) -> Result<(Reply, bool), Diagnostic> {
        let root = self.root.clone();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .db()?;
        // A request waiting on the writer lock must not renew using an old time.
        let sampled = self.clock.now_ms()?;
        if sampled < 0 {
            return Err(error(ErrorCode::Blocked));
        }
        authenticate(&tx, actor)?;
        if actor.id != request.actor_id {
            return Err(error(ErrorCode::Unauthorized));
        }
        let fingerprint = if let Some(launch) = activation {
            launch.fingerprint(request)?
        } else if let Some(proof) = native {
            proof.fingerprint(request)?
        } else {
            request_fingerprint(request, local)?
        };
        let reserved: Option<(String, String)> = tx
            .query_row(
                "SELECT actor_id,fingerprint FROM restore_intents WHERE operation_id=?1 UNION ALL
                 SELECT json_extract(record,'$.binding.receiver_id'),json_extract(record,'$.receiver_stage.fingerprint')
                 FROM transfers WHERE json_extract(record,'$.receiver_stage.operation_id')=?1",
                [request.operation_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .db()?;
        if reserved.is_some_and(|binding| binding != (actor.id.to_string(), fingerprint.clone())) {
            audit(&tx, request, sampled, Some(ErrorCode::IdempotencyConflict))?;
            tx.commit().db()?;
            return Err(error(ErrorCode::IdempotencyConflict));
        }
        let cached: Option<(String, String, String)> = tx
            .query_row(
                "SELECT actor_id, fingerprint, result FROM operations WHERE id=?1",
                [request.operation_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .db()?;
        if let Some((owner, hash, result)) = cached {
            if owner == actor.id.to_string() && hash == fingerprint {
                let result: Result<Reply, Diagnostic> = decode(&result)?;
                if let Ok(ref reply) = result {
                    if let Err(e) = snapshots::verify_reply(&tx, &root, request, reply) {
                        audit(&tx, request, sampled, Some(e.code))?;
                        tx.commit().db()?;
                        return Err(e);
                    }
                }
                return result.map(|reply| (reply, false));
            }
            audit(&tx, request, sampled, Some(ErrorCode::IdempotencyConflict))?;
            tx.commit().db()?;
            return Err(error(ErrorCode::IdempotencyConflict));
        }
        let last: i64 = tx
            .query_row("SELECT last_ms FROM clock WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .db()?;
        let now = sampled.max(last);
        tx.execute("UPDATE clock SET last_ms=?1 WHERE singleton=1", [now])
            .db()?;
        let result = validate(request).and_then(|_| {
            if local && !local_mutation_allowed(&request.action) {
                Err(error(ErrorCode::Forbidden))
            } else if (matches!(request.action, Action::AcknowledgeReceiver { .. })
                && !matches!(native, Some(native::Evidence::Ready(_))))
                || (matches!(
                    request.action,
                    Action::ObserveRun {
                        observation: RunObservation::ToolReserved { .. },
                        ..
                    }
                ) && !matches!(native, Some(native::Evidence::Tool(_))))
            {
                Err(error(ErrorCode::ExecutionNotEnabled))
            } else {
                if let Some(proof) = native {
                    proof.validate(&tx, &root, request)?;
                }
                let reply = apply(
                    &tx,
                    &root,
                    request,
                    now,
                    self.clock.as_ref(),
                    local,
                    activation,
                )?;
                if let Some(proof) = native {
                    proof.persist(&tx, request, &reply, now)?;
                }
                if matches!(native, Some(native::Evidence::Tool(_))) {
                    let mut task = load_task(
                        &tx,
                        request
                            .action
                            .task_id()
                            .ok_or_else(|| error(ErrorCode::InvalidInput))?,
                    )?;
                    snapshots::finish(&tx, request, &mut task, now, self.clock.as_ref())?;
                }
                if let Action::ObserveRun {
                    task_id,
                    observation:
                        RunObservation::ToolStarted { .. }
                        | RunObservation::Spawned { .. }
                        | RunObservation::AdapterReported {
                            event:
                                AdapterEvent {
                                    event:
                                        AdapterEventKind::ToolStarted { .. }
                                        | AdapterEventKind::TurnStarted,
                                    ..
                                },
                            ..
                        },
                    ..
                } = request.action
                {
                    let mut task = load_task(&tx, task_id)?;
                    if matches!(task.state, TaskState::Reserved | TaskState::Quiescing)
                        && transfers::pending_source(&tx, &task)?.is_some()
                    {
                        transfers::source_lost(&tx, request, &mut task, now)?;
                    }
                }
                if let Action::ObserveRun {
                    task_id,
                    run_id,
                    observation: RunObservation::SupervisorLost | RunObservation::TreeStopped,
                    ..
                } = request.action
                {
                    let mut task = load_task(&tx, task_id)?;
                    if task.state == TaskState::Quiescing
                        && matches!(
                            request.action,
                            Action::ObserveRun {
                                observation: RunObservation::SupervisorLost,
                                ..
                            }
                        )
                    {
                        transfers::source_lost(&tx, request, &mut task, now)?;
                    }
                    if matches!(
                        task.state,
                        TaskState::Activating | TaskState::Active | TaskState::RecoveryRequired
                    ) {
                        transfers::receiver_lost(&tx, request, &mut task, run_id, now)?;
                    }
                }
                Ok(reply)
            }
        });
        if let Err(ref e) = result {
            if matches!(
                e.code,
                ErrorCode::Io
                    | ErrorCode::StoreFailure
                    | ErrorCode::StoreBusy
                    | ErrorCode::CorruptStore
            ) {
                return result.map(|reply| (reply, false));
            }
        }
        audit(&tx, request, now, result.as_ref().err().map(|e| e.code))?;
        tx.execute(
            "INSERT INTO operations VALUES (?1, ?2, ?3, ?4)",
            params![
                request.operation_id.to_string(),
                actor.id.to_string(),
                fingerprint,
                encode(&result)?
            ],
        )
        .db()?;
        #[cfg(test)]
        crash_point("before_commit");
        #[cfg(test)]
        if matches!(native, Some(native::Evidence::Spawn(..))) {
            crash_point("native_before_commit");
        }
        #[cfg(test)]
        if matches!(native, Some(native::Evidence::Ready(_))) {
            crash_point("receiver_ready_before_commit");
        }
        tx.commit().db()?;
        #[cfg(test)]
        crash_point("after_commit");
        #[cfg(test)]
        if matches!(native, Some(native::Evidence::Spawn(..))) {
            crash_point("native_after_commit");
        }
        #[cfg(test)]
        if matches!(native, Some(native::Evidence::Ready(_))) {
            crash_point("receiver_ready_after_commit");
        }
        result.map(|reply| (reply, true))
    }

    pub fn task(&self, actor: &Actor, task: Id) -> Result<Task, Diagnostic> {
        let tx = self.connection.unchecked_transaction().db()?;
        read_task(&tx, actor, task)
    }

    pub fn transfer(
        &self,
        actor: &Actor,
        task: Id,
        transfer: Id,
    ) -> Result<TransferRecord, Diagnostic> {
        let tx = self.connection.unchecked_transaction().db()?;
        authenticate(&tx, actor)?;
        access(&tx, actor.id, task)?;
        transfers::record(&tx, task, transfer)
    }

    pub fn tasks_page(
        &self,
        actor: &Actor,
        after: Option<Id>,
        limit: u16,
    ) -> Result<Vec<Task>, Diagnostic> {
        if !(1..=100).contains(&limit) {
            return Err(error(ErrorCode::InvalidInput));
        }
        let tx = self.connection.unchecked_transaction().db()?;
        authenticate(&tx, actor)?;
        let mut statement = tx.prepare("SELECT task_id FROM task_access WHERE actor_id=?1 AND task_id>?2 ORDER BY task_id LIMIT ?3").db()?;
        let rows = statement
            .query_map(
                params![
                    actor.id.to_string(),
                    after.map(|id| id.to_string()).unwrap_or_default(),
                    limit
                ],
                |r| r.get::<_, String>(0),
            )
            .db()?;
        rows.map(|row| {
            let id = Id::parse_str(&row.db()?).map_err(|_| error(ErrorCode::CorruptStore))?;
            read_task(&tx, actor, id)
        })
        .collect()
    }

    pub fn events_page(
        &self,
        actor: &Actor,
        task: Id,
        after: u64,
        limit: u16,
    ) -> Result<Vec<Event>, Diagnostic> {
        if !(1..=200).contains(&limit) {
            return Err(error(ErrorCode::InvalidInput));
        }
        let tx = self.connection.unchecked_transaction().db()?;
        let record = read_task(&tx, actor, task)?;
        if after > record.last_event_sequence {
            return Err(error(ErrorCode::InvalidInput));
        }
        let mut statement = tx.prepare("SELECT record FROM events WHERE task_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3").db()?;
        let rows = statement
            .query_map(params![task.to_string(), after as i64, limit], |r| {
                r.get::<_, String>(0)
            })
            .db()?;
        let mut events = Vec::new();
        for row in rows {
            let event: Event = decode(&row.db()?)?;
            if event.task_id != task || event.sequence != after + events.len() as u64 + 1 {
                return Err(error(ErrorCode::CorruptStore));
            }
            events.push(event);
        }
        Ok(events)
    }
    pub fn checkpoint(
        &self,
        actor: &Actor,
        task: Id,
        checkpoint: Id,
    ) -> Result<CheckpointRecord, Diagnostic> {
        let tx = self.connection.unchecked_transaction().db()?;
        read_task(&tx, actor, task)?;
        snapshots::record(&tx, &self.root, task, checkpoint)
    }

    pub fn events(&self, actor: &Actor, task: Id) -> Result<Vec<Event>, Diagnostic> {
        let tx = self.connection.unchecked_transaction().db()?;
        read_task(&tx, actor, task)?;
        let mut statement = tx
            .prepare("SELECT record FROM events WHERE task_id=?1 ORDER BY sequence")
            .db()?;
        let rows = statement
            .query_map([task.to_string()], |r| r.get::<_, String>(0))
            .db()?;
        let mut events = Vec::new();
        for row in rows {
            let event: Event = decode(&row.db()?)?;
            if event.task_id != task || event.sequence != events.len() as u64 + 1 {
                return Err(error(ErrorCode::CorruptStore));
            }
            events.push(event);
        }
        Ok(events)
    }

    pub fn audit(&self, actor: &Actor, task: Id) -> Result<Vec<AuditEntry>, Diagnostic> {
        let tx = self.connection.unchecked_transaction().db()?;
        read_task(&tx, actor, task)?;
        let mut statement = tx.prepare(
            "SELECT record FROM audit WHERE json_extract(record, '$.task_id')=?1 ORDER BY sequence").db()?;
        let rows = statement
            .query_map([task.to_string()], |r| r.get::<_, String>(0))
            .db()?;
        rows.map(|r| decode(&r.db()?)).collect()
    }
}

fn connect(root: &Path) -> Result<Connection, Diagnostic> {
    baton_platform::validate_private(root)?;
    let metadata = fs::symlink_metadata(root).map_err(|_| error(ErrorCode::Io))?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(error(ErrorCode::InvalidInput));
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        match fs::symlink_metadata(root.join(format!("{FILE}{suffix}"))) {
            Ok(meta) if !meta.is_file() || is_link(&meta) => {
                return Err(error(ErrorCode::InvalidInput))
            }
            Ok(meta) => {
                baton_platform::validate_private(&root.join(format!("{FILE}{suffix}")))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if meta.nlink() != 1 {
                        return Err(error(ErrorCode::InvalidInput));
                    }
                }
                #[cfg(not(unix))]
                let _ = meta;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !suffix.is_empty() => (),
            Err(_) => return Err(error(ErrorCode::Io)),
        }
    }
    let connection = Connection::open_with_flags(
        root.join(FILE),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .db()?;
    connection.busy_timeout(Duration::from_secs(10)).db()?;
    Ok(connection)
}
fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
fn configure(c: &Connection) -> Result<(), Diagnostic> {
    if rusqlite::version_number() < 3_051_003 {
        return Err(error(ErrorCode::UnsupportedSchema));
    }
    let mode: String = c
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .db()?;
    c.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
        .db()?;
    let sync: i64 = c
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .db()?;
    let foreign_keys: i64 = c
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .db()?;
    if mode != "wal" || sync != 2 || foreign_keys != 1 {
        return Err(error(ErrorCode::Blocked));
    }
    Ok(())
}
fn authenticate(c: &Connection, actor: &Actor) -> Result<(), Diagnostic> {
    let known = c
        .prepare("SELECT 1 FROM actors WHERE id=?1 AND token_hash=?2")
        .db()?
        .exists(params![actor.id.to_string(), actor.hash()])
        .db()?;
    if known {
        Ok(())
    } else {
        Err(error(ErrorCode::Unauthorized))
    }
}
fn access(c: &Connection, actor: Id, task: Id) -> Result<(), Diagnostic> {
    if c.prepare("SELECT 1 FROM task_access WHERE task_id=?1 AND actor_id=?2")
        .db()?
        .exists(params![task.to_string(), actor.to_string()])
        .db()?
    {
        Ok(())
    } else {
        Err(error(ErrorCode::Forbidden))
    }
}
fn load_task(c: &Connection, id: Id) -> Result<Task, Diagnostic> {
    let json: String = c
        .query_row(
            "SELECT record FROM tasks WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .db()?
        .ok_or_else(|| error(ErrorCode::NotFound))?;
    let task: Task = decode(&json)?;
    let owned = task.owner_id.is_some() && task.lease_id.is_some() && task.expires_at_ms.is_some();
    let unowned =
        task.owner_id.is_none() && task.lease_id.is_none() && task.expires_at_ms.is_none();
    if task.task_id != id
        || task.epoch > i64::MAX as u64
        || (task.state == TaskState::Unclaimed && !unowned)
        || (task.state != TaskState::Unclaimed && (!owned || task.epoch == 0))
    {
        return Err(error(ErrorCode::CorruptStore));
    }
    let (count, max): (i64, i64) = c
        .query_row(
            "SELECT count(*), coalesce(max(sequence),0) FROM events WHERE task_id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .db()?;
    if count == 0 || count != max || u64::try_from(max).ok() != Some(task.last_event_sequence) {
        return Err(error(ErrorCode::CorruptStore));
    }
    let mut grants = c.prepare("SELECT id FROM transfers WHERE task_id=?1 AND json_extract(record,'$.committed_epoch') IS NOT NULL").db()?;
    let mut matched = false;
    for grant in grants
        .query_map([id.to_string()], |r| r.get::<_, String>(0))
        .db()?
    {
        let grant = Id::parse_str(&grant.db()?).map_err(|_| error(ErrorCode::CorruptStore))?;
        let grant = transfers::record(c, id, grant)?;
        let epoch = grant
            .committed_epoch
            .ok_or_else(|| error(ErrorCode::CorruptStore))?;
        if epoch > task.epoch {
            return Err(error(ErrorCode::CorruptStore));
        }
        if epoch == task.epoch {
            matched = true;
            if task.owner_id != Some(grant.binding.receiver_id)
                || (task.state == TaskState::Active && grant.state != TransferState::Active)
                || (task.state == TaskState::Quiescing && grant.state != TransferState::Active)
                || (task.state == TaskState::Activating && grant.state != TransferState::Activating)
                || (task.state == TaskState::Committed && grant.state != TransferState::Committed)
                || grant
                    .receiver_activation
                    .as_ref()
                    .is_some_and(|run| task.lease_id != Some(run.binding.lease_id))
                || !matches!(
                    task.state,
                    TaskState::Committed
                        | TaskState::Activating
                        | TaskState::RecoveryRequired
                        | TaskState::Active
                        | TaskState::Quiescing
                )
            {
                return Err(error(ErrorCode::CorruptStore));
            }
        }
    }
    if matches!(
        task.state,
        TaskState::Committed
            | TaskState::Activating
            | TaskState::RecoveryRequired
            | TaskState::Active
    ) && !matched
    {
        return Err(error(ErrorCode::CorruptStore));
    }
    Ok(task)
}
fn expired_state(c: &Connection, task: &Task) -> Result<TaskState, Diagnostic> {
    Ok(if transfers::grant(c, task)?.is_some() {
        TaskState::RecoveryRequired
    } else {
        TaskState::Suspect
    })
}
fn read_task(c: &Connection, actor: &Actor, task: Id) -> Result<Task, Diagnostic> {
    authenticate(c, actor)?;
    access(c, actor.id, task)?;
    load_task(c, task)
}
fn save_task(c: &Connection, task: &Task, epoch: u64) -> Result<(), Diagnostic> {
    let epoch = i64::try_from(epoch).map_err(|_| error(ErrorCode::InvalidInput))?;
    let changed = c
        .execute(
            "UPDATE tasks SET record=?1 WHERE id=?2 AND json_extract(record, '$.epoch')=?3",
            params![encode(task)?, task.task_id.to_string(), epoch],
        )
        .db()?;
    if changed != 1 {
        return Err(error(ErrorCode::StaleEpoch));
    }
    Ok(())
}
fn text(value: &str, max: usize) -> Result<(), Diagnostic> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(error(ErrorCode::InvalidInput))
    } else {
        Ok(())
    }
}
fn request_fingerprint(r: &Request, local: bool) -> Result<String, Diagnostic> {
    let mut hash = blake3::Hasher::new();
    hash.update(if local { b"local-v1:" } else { b"trusted-v1:" });
    hash.update(encode(r)?.as_bytes());
    Ok(hash.finalize().to_hex().to_string())
}
fn validate(r: &Request) -> Result<(), Diagnostic> {
    if r.actor_id.is_nil()
        || r.operation_id.is_nil()
        || r.expected_epoch > i64::MAX as u64
        || r.action.task_id().is_some_and(|id| id.is_nil())
    {
        return Err(error(ErrorCode::InvalidInput));
    }
    match &r.action {
        Action::EnrollRepository {
            root,
            repository_id,
        } => {
            text(root, 32_768)?;
            if repository_id.is_nil() || r.expected_epoch != 0 {
                return Err(error(ErrorCode::InvalidInput));
            }
        }
        Action::CreateTask {
            goal,
            scope,
            agent,
            repository_id,
            ..
        } => {
            text(goal, 16_384)?;
            text(scope, 4_096)?;
            text(&agent.name, 256)?;
            text(&agent.version, 256)?;
            if repository_id.is_nil() || r.expected_epoch != 0 {
                return Err(error(ErrorCode::InvalidInput));
            }
        }
        Action::ClaimTask { lease_ms, .. }
        | Action::Heartbeat { lease_ms, .. }
        | Action::CommitTransfer { lease_ms, .. }
            if !(1..=300_000).contains(lease_ms) =>
        {
            return Err(error(ErrorCode::InvalidInput))
        }
        Action::RecordIntent { summary, .. } => text(summary, 4_096)?,
        Action::RestoreCheckpoint {
            restore_id,
            checkpoint_id,
            ..
        } if restore_id.is_nil() || checkpoint_id.is_nil() => {
            return Err(error(ErrorCode::InvalidInput));
        }
        _ => (),
    }
    Ok(())
}
fn append_event(
    c: &Connection,
    r: &Request,
    task: &mut Task,
    now: i64,
    kind: EventKind,
    intent: Option<String>,
    artifact: Option<&str>,
) -> Result<Id, Diagnostic> {
    let sequence = task
        .last_event_sequence
        .checked_add(1)
        .and_then(|n| i64::try_from(n).ok())
        .ok_or_else(|| error(ErrorCode::CorruptStore))?;
    let event = Event {
        schema_version: SchemaVersion::V0_1_0,
        event_id: Id::new_v4(),
        task_id: task.task_id,
        sequence: u64::try_from(sequence).map_err(|_| error(ErrorCode::CorruptStore))?,
        actor_id: r.actor_id,
        epoch: task.epoch,
        operation_id: r.operation_id,
        timestamp_ms: now,
        origin: if intent.is_some() {
            EventOrigin::AgentReported
        } else {
            EventOrigin::Coordinator
        },
        kind,
        reported_intent: intent,
        evidence_hash: artifact.map(str::to_owned),
    };
    c.execute(
        "INSERT INTO events VALUES (?1, ?2, ?3, ?4)",
        params![
            task.task_id.to_string(),
            sequence,
            event.event_id.to_string(),
            encode(&event)?
        ],
    )
    .db()?;
    task.last_event_sequence = event.sequence;
    save_task(c, task, task.epoch)?;
    Ok(event.event_id)
}
fn audit(
    c: &Connection,
    r: &Request,
    now: i64,
    reason: Option<ErrorCode>,
) -> Result<(), Diagnostic> {
    let entry = AuditEntry {
        schema_version: SchemaVersion::V0_1_0,
        audit_id: Id::new_v4(),
        actor_id: r.actor_id,
        operation_id: r.operation_id,
        task_id: r.action.task_id(),
        expected_epoch: r.expected_epoch,
        timestamp_ms: now,
        reason,
        kind: AuditKind::Request,
    };
    c.execute("INSERT INTO audit(record) VALUES (?1)", [encode(&entry)?])
        .db()?;
    Ok(())
}

fn apply(
    c: &Connection,
    state_root: &Path,
    r: &Request,
    now: i64,
    clock: &dyn Clock,
    local: bool,
    activation: Option<&execution::ReceiverLaunch<'_>>,
) -> Result<Reply, Diagnostic> {
    if let Action::EnrollRepository {
        repository_id,
        root,
    } = &r.action
    {
        let (root, git_dir) = identify(Path::new(root))?;
        if state_root.starts_with(&root) || state_root.starts_with(&git_dir) {
            return Err(error(ErrorCode::InvalidInput));
        }
        let root = root
            .to_str()
            .ok_or_else(|| error(ErrorCode::InvalidInput))?;
        let git_dir = git_dir
            .to_str()
            .ok_or_else(|| error(ErrorCode::InvalidInput))?;
        let existing: Option<(String, String)> = c
            .query_row(
                "SELECT id, created_by FROM repositories WHERE root=?1 AND git_dir=?2",
                params![root, git_dir],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .db()?;
        if let Some((id, owner)) = existing {
            if owner != r.actor_id.to_string() {
                return Err(error(ErrorCode::Forbidden));
            }
            return Ok(Reply::Repository(
                Id::parse_str(&id).map_err(|_| error(ErrorCode::CorruptStore))?,
            ));
        }
        if c.prepare("SELECT 1 FROM repositories WHERE id=?1")
            .db()?
            .exists([repository_id.to_string()])
            .db()?
        {
            return Err(error(ErrorCode::InvalidInput));
        }
        c.execute(
            "INSERT INTO repositories VALUES (?1, ?2, ?3, ?4)",
            params![
                repository_id.to_string(),
                root,
                git_dir,
                r.actor_id.to_string()
            ],
        )
        .db()?;
        return Ok(Reply::Repository(*repository_id));
    }
    if let Action::CreateTask {
        task_id,
        repository_id,
        goal,
        scope,
        agent,
    } = &r.action
    {
        let owner: Option<String> = c
            .query_row(
                "SELECT created_by FROM repositories WHERE id=?1",
                [repository_id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .db()?;
        if owner.as_deref() != Some(&r.actor_id.to_string()) {
            return Err(error(ErrorCode::Forbidden));
        }
        if c.prepare("SELECT 1 FROM tasks WHERE id=?1")
            .db()?
            .exists([task_id.to_string()])
            .db()?
        {
            return Err(error(ErrorCode::InvalidInput));
        }
        let mut task = Task {
            schema_version: SchemaVersion::V0_1_0,
            task_id: *task_id,
            repository_id: *repository_id,
            created_by: r.actor_id,
            goal: goal.clone(),
            scope: scope.clone(),
            agent: agent.clone(),
            epoch: 0,
            last_event_sequence: 0,
            state: TaskState::Unclaimed,
            owner_id: None,
            lease_id: None,
            expires_at_ms: None,
        };
        c.execute(
            "INSERT INTO tasks VALUES (?1, ?2, ?3)",
            params![
                task_id.to_string(),
                repository_id.to_string(),
                encode(&task)?
            ],
        )
        .db()?;
        c.execute(
            "INSERT INTO task_access VALUES (?1, ?2)",
            params![task_id.to_string(), r.actor_id.to_string()],
        )
        .db()?;
        append_event(c, r, &mut task, now, EventKind::TaskCreated, None, None)?;
        return Ok(Reply::Task(task));
    }
    let id = r
        .action
        .task_id()
        .ok_or_else(|| error(ErrorCode::InvalidInput))?;
    access(c, r.actor_id, id)?;
    let mut task = load_task(c, id)?;
    if task.epoch != r.expected_epoch {
        return Err(error(ErrorCode::StaleEpoch));
    }
    if matches!(
        task.state,
        TaskState::Reserved
            | TaskState::Committed
            | TaskState::Activating
            | TaskState::Active
            | TaskState::Quiescing
    ) && task.expires_at_ms.is_some_and(|expiry| now >= expiry)
    {
        task.state = expired_state(c, &task)?;
        save_task(c, &task, task.epoch)?;
        append_event(c, r, &mut task, now, EventKind::LeaseSuspect, None, None)?;
    }
    if let Action::GrantAccess { actor_id, .. } = r.action {
        if task.created_by != r.actor_id {
            return Err(error(ErrorCode::Forbidden));
        }
        if !c
            .prepare("SELECT 1 FROM actors WHERE id=?1")
            .db()?
            .exists([actor_id.to_string()])
            .db()?
        {
            return Err(error(ErrorCode::NotFound));
        }
        c.execute(
            "INSERT OR IGNORE INTO task_access VALUES (?1, ?2)",
            params![id.to_string(), actor_id.to_string()],
        )
        .db()?;
        append_event(c, r, &mut task, now, EventKind::AccessGranted, None, None)?;
        return Ok(Reply::AccessGranted);
    }
    let reporting_loss = matches!(
        r.action,
        Action::ObserveRun {
            observation: RunObservation::SupervisorLost,
            ..
        }
    );
    // Aborting intent never activates a writer or restores an expired reservation.
    if matches!(r.action, Action::AbortTransfer { .. }) {
        return transfers::apply(c, state_root, r, &mut task, now, local, clock);
    }
    if matches!(r.action, Action::RecoverTransfer { .. }) {
        return transfers::apply(c, state_root, r, &mut task, now, local, clock);
    }
    if let Some(launch) = activation {
        return launch.reserve(c, state_root, r, &mut task, now, clock);
    }
    if task.state == TaskState::Suspect && !reporting_loss {
        return Err(error(ErrorCode::Blocked));
    }
    if matches!(
        r.action,
        Action::BeginReceiverStaging { .. }
            | Action::StageReceiver { .. }
            | Action::CommitTransfer { .. }
            | Action::ActivateReceiver { .. }
            | Action::AcknowledgeReceiver { .. }
    ) {
        return transfers::apply(c, state_root, r, &mut task, now, local, clock);
    }
    if let Action::ClaimTask { lease_ms, .. } = r.action {
        if task.state != TaskState::Unclaimed {
            return Err(error(ErrorCode::Blocked));
        }
        let epoch = task
            .epoch
            .checked_add(1)
            .filter(|e| *e <= i64::MAX as u64)
            .ok_or_else(|| error(ErrorCode::Blocked))?;
        let expiry = now
            .checked_add(lease_ms)
            .ok_or_else(|| error(ErrorCode::Blocked))?;
        task.epoch = epoch;
        task.state = TaskState::Reserved;
        task.owner_id = Some(r.actor_id);
        task.lease_id = Some(Id::new_v4());
        task.expires_at_ms = Some(expiry);
        save_task(c, &task, r.expected_epoch)?;
        append_event(c, r, &mut task, now, EventKind::Claimed, None, None)?;
        return Ok(Reply::Lease(lease(&task)?));
    }
    let request_lease = match r.action {
        Action::Heartbeat { lease_id, .. }
        | Action::CancelReservation { lease_id, .. }
        | Action::RecordIntent { lease_id, .. }
        | Action::CaptureCheckpoint { lease_id, .. }
        | Action::RestoreCheckpoint { lease_id, .. }
        | Action::CollectOrphans { lease_id, .. } => lease_id,
        Action::OpenRun { lease_id, .. } | Action::ObserveRun { lease_id, .. } => lease_id,
        Action::RequestTransfer { lease_id, .. } | Action::PreflightTransfer { lease_id, .. } => {
            lease_id
        }
        _ => return Err(error(ErrorCode::InvalidInput)),
    };
    if task.state == TaskState::Quiescing
        && matches!(
            r.action,
            Action::ObserveRun {
                observation: RunObservation::ToolReserved { .. },
                ..
            }
        )
    {
        return Err(error(ErrorCode::Blocked));
    }
    let receiver_observation = (task.state == TaskState::Activating
        && matches!(
            r.action,
            Action::ObserveRun { .. } | Action::Heartbeat { .. }
        ))
        || (task.state == TaskState::Active
            && matches!(
                r.action,
                Action::ObserveRun { .. }
                    | Action::Heartbeat { .. }
                    | Action::CaptureCheckpoint { .. }
                    | Action::RecordIntent { .. }
                    | Action::RequestTransfer { .. }
            ))
        || (task.state == TaskState::Quiescing
            && matches!(
                r.action,
                Action::ObserveRun { .. }
                    | Action::Heartbeat { .. }
                    | Action::CaptureCheckpoint { .. }
                    | Action::RecordIntent { .. }
                    | Action::PreflightTransfer { .. }
                    | Action::RequestTransfer { .. }
            ));
    if (task.state != TaskState::Reserved
        && !receiver_observation
        && !(reporting_loss
            && matches!(task.state, TaskState::Suspect | TaskState::RecoveryRequired)))
        || task.owner_id != Some(r.actor_id)
        || task.lease_id != Some(request_lease)
    {
        return Err(error(ErrorCode::Forbidden));
    }
    if matches!(
        r.action,
        Action::OpenRun { .. } | Action::CancelReservation { .. }
    ) && transfers::pending(c, id)?
    {
        return Err(error(ErrorCode::Blocked));
    }
    match &r.action {
        Action::Heartbeat { lease_ms, .. } => {
            task.expires_at_ms = Some(
                now.checked_add(*lease_ms)
                    .ok_or_else(|| error(ErrorCode::Blocked))?,
            );
            save_task(c, &task, r.expected_epoch)?;
            append_event(c, r, &mut task, now, EventKind::Heartbeat, None, None)?;
            Ok(Reply::Lease(lease(&task)?))
        }
        Action::CancelReservation { .. } => {
            if c.prepare("SELECT 1 FROM runs WHERE task_id=?1")
                .db()?
                .exists([id.to_string()])
                .db()?
            {
                return Err(error(ErrorCode::Blocked));
            }
            task.state = TaskState::Unclaimed;
            task.owner_id = None;
            task.lease_id = None;
            task.expires_at_ms = None;
            save_task(c, &task, r.expected_epoch)?;
            append_event(
                c,
                r,
                &mut task,
                now,
                EventKind::ReservationCancelled,
                None,
                None,
            )?;
            Ok(Reply::Task(task))
        }
        Action::RecordIntent { summary, .. } => Ok(Reply::Recorded(append_event(
            c,
            r,
            &mut task,
            now,
            EventKind::ReportedIntent,
            Some(summary.clone()),
            None,
        )?)),
        Action::OpenRun { .. } | Action::ObserveRun { .. } => {
            execution::apply(c, r, &mut task, now)
        }
        Action::RequestTransfer { .. } | Action::PreflightTransfer { .. } => {
            transfers::apply(c, state_root, r, &mut task, now, local, clock)
        }
        _ => snapshots::apply(c, state_root, r, &mut task, now, clock, local),
    }
}
fn lease(task: &Task) -> Result<Lease, Diagnostic> {
    Ok(Lease {
        schema_version: SchemaVersion::V0_1_0,
        task_id: task.task_id,
        epoch: task.epoch,
        lease_id: task
            .lease_id
            .ok_or_else(|| error(ErrorCode::CorruptStore))?,
        owner_id: task
            .owner_id
            .ok_or_else(|| error(ErrorCode::CorruptStore))?,
        expires_at_ms: task
            .expires_at_ms
            .ok_or_else(|| error(ErrorCode::CorruptStore))?,
    })
}

fn git(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // Background Git must not open a console.
    }
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(key);
        }
    }
    command
        .current_dir(cwd)
        .args(["--no-optional-locks", "-c", "core.fsmonitor=false"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    command
}
fn run_git(cwd: &Path, args: &[&str]) -> Result<std::process::Output, Diagnostic> {
    git(cwd).args(args).output().map_err(|e| {
        error(if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::GitUnavailable
        } else {
            ErrorCode::Io
        })
    })
}
fn ensure_outside_git(parent: &Path) -> Result<(), Diagnostic> {
    let output = run_git(parent, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() && output.stderr.starts_with(b"fatal: not a git repository") {
        Ok(())
    } else {
        Err(error(ErrorCode::InvalidInput))
    }
}
fn identify(path: &Path) -> Result<(PathBuf, PathBuf), Diagnostic> {
    if !path.is_absolute() {
        return Err(error(ErrorCode::InvalidInput));
    }
    let path = fs::canonicalize(path).map_err(|_| error(ErrorCode::Io))?;
    let mut result = Vec::new();
    for arg in ["--show-toplevel", "--git-common-dir"] {
        let output = run_git(&path, &["rev-parse", "--path-format=absolute", arg])?;
        if !output.status.success() {
            return Err(error(ErrorCode::GitFailed));
        }
        let raw =
            std::str::from_utf8(&output.stdout).map_err(|_| error(ErrorCode::InvalidInput))?;
        let value = raw.strip_suffix('\n').unwrap_or(raw);
        let value = value.strip_suffix('\r').unwrap_or(value);
        if value.contains(['\n', '\r', '\0']) {
            return Err(error(ErrorCode::InvalidInput));
        }
        let canonical = fs::canonicalize(value).map_err(|_| error(ErrorCode::Io))?;
        result.push(canonical);
    }
    Ok((result.remove(0), result.remove(0)))
}

#[cfg(test)]
fn crash_point(point: &str) {
    if std::env::var("BATON_STORE_TEST_CRASH").as_deref() == Ok(point) {
        let ready = fs::File::create(std::env::var_os("BATON_STORE_TEST_READY").unwrap()).unwrap();
        ready.sync_all().unwrap();
        // The parent kills this process at an observed boundary, without cleanup.
        loop {
            std::thread::park();
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod transfer_tests;
