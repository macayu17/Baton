use super::*;
use baton_platform::process::{NativeDrainProof, NativeRunningProof, ProcessTree};

#[derive(Clone, Copy)]
pub(super) enum Evidence<'a> {
    Spawn(&'a ProcessTree, &'a LaunchPermit),
    Drain(&'a NativeDrainProof<'a>),
    Ready(&'a NativeRunningProof<'a>),
    Tool(&'a NativeRunningProof<'a>),
}
impl Evidence<'_> {
    fn identity(self) -> (Id, u32, String) {
        match self {
            Self::Spawn(tree, _) => (
                tree.scope_id(),
                tree.pid(),
                tree.launch_fingerprint().into(),
            ),
            Self::Drain(proof) => (
                proof.scope_id(),
                proof.pid(),
                proof.launch_fingerprint().into(),
            ),
            Self::Ready(proof) | Self::Tool(proof) => (
                proof.scope_id(),
                proof.pid(),
                proof.launch_fingerprint().into(),
            ),
        }
    }
    pub(super) fn fingerprint(self, r: &Request) -> Result<String, Diagnostic> {
        let mut hash = blake3::Hasher::new();
        hash.update(b"native-scope-v1:");
        hash.update(self.identity().0.as_bytes());
        hash.update(encode(r)?.as_bytes());
        Ok(hash.finalize().to_hex().to_string())
    }
    pub(super) fn validate(
        self,
        c: &Connection,
        root: &Path,
        r: &Request,
    ) -> Result<(), Diagnostic> {
        if let (
            Self::Tool(proof),
            Action::ObserveRun {
                task_id,
                run_id,
                lease_id,
                observation:
                    RunObservation::ToolReserved {
                        source_sequence,
                        class,
                        ..
                    },
            },
        ) = (self, &r.action)
        {
            let binding = RunBinding {
                task_id: *task_id,
                run_id: *run_id,
                lease_id: *lease_id,
                actor_id: r.actor_id,
                epoch: r.expected_epoch,
            };
            let (_, run) = tool_run(c, root, &binding, proof)?;
            if !matches!(class, EffectClass::ReadOnly | EffectClass::LocalReversible)
                || run.source_cursor.checked_add(1) != Some(*source_sequence)
                || run.blockers().contains(&BoundaryReason::UnresolvedEffect)
            {
                return Err(error(ErrorCode::Blocked));
            }
            return Ok(());
        }
        if let (
            Self::Ready(_),
            Action::AcknowledgeReceiver {
                task_id, session, ..
            },
        ) = (self, &r.action)
        {
            let run = execution::load(c, *task_id, session.binding.run_id)?;
            let scope = scope_record(c, *task_id, run.binding.run_id)?;
            let (scope_id, pid, fingerprint) = self.identity();
            if session.binding != run.binding
                || run.binding.actor_id != r.actor_id
                || run.binding.epoch != r.expected_epoch
                || scope.binding != run.binding
                || scope.scope_id != scope_id
                || scope.pid != pid
                || scope.launch_fingerprint != fingerprint
                || run.launch_fingerprint != fingerprint
                || run.pid != Some(pid)
            {
                return Err(error(ErrorCode::Forbidden));
            }
            if run.state != RunState::Running
                || run.source_cursor != 0
                || run.event_gap
                || !run.effects.is_empty()
            {
                return Err(error(ErrorCode::Blocked));
            }
            return Ok(());
        }
        let Action::ObserveRun {
            task_id,
            run_id,
            lease_id,
            ref observation,
        } = r.action
        else {
            return Err(error(ErrorCode::Forbidden));
        };
        let run = execution::load(c, task_id, run_id)?;
        let (scope, pid, fingerprint) = self.identity();
        if run.binding.actor_id != r.actor_id
            || run.binding.lease_id != lease_id
            || run.binding.epoch != r.expected_epoch
            || run.launch_fingerprint != fingerprint
        {
            return Err(error(ErrorCode::Forbidden));
        }
        match (self, observation) {
            (Self::Spawn(_, permit), RunObservation::Spawned { pid: requested })
                if *requested == pid =>
            {
                if run != *permit.record() || !permit.matches_grant(c)? {
                    return Err(error(ErrorCode::Blocked));
                }
            }
            (Self::Drain(_), RunObservation::TreeStopped) => {
                let bound = scope_record(c, task_id, run_id)?;
                if bound.scope_id != scope
                    || bound.pid != pid
                    || bound.binding != run.binding
                    || bound.launch_fingerprint != fingerprint
                {
                    return Err(error(ErrorCode::Forbidden));
                }
            }
            _ => return Err(error(ErrorCode::Forbidden)),
        }
        Ok(())
    }
    pub(super) fn persist(
        self,
        c: &Connection,
        r: &Request,
        reply: &Reply,
        now: i64,
    ) -> Result<(), Diagnostic> {
        if matches!(self, Self::Ready(_) | Self::Tool(_)) {
            return if matches!(
                (self, reply),
                (Self::Ready(_), Reply::Transfer(_)) | (Self::Tool(_), Reply::Run(_))
            ) {
                Ok(())
            } else {
                Err(error(ErrorCode::CorruptStore))
            };
        }
        let Reply::Run(run) = reply else {
            return Err(error(ErrorCode::CorruptStore));
        };
        let mut task = load_task(c, run.binding.task_id)?;
        let (scope_id, pid, fingerprint) = self.identity();
        let (json, kind) = match self {
            Self::Spawn(tree, _) => (
                encode(&NativeScopeRecord {
                    schema_version: SchemaVersion::V0_1_0,
                    binding: run.binding.clone(),
                    scope_id,
                    launch_fingerprint: fingerprint,
                    pid,
                    launch_worktree: tree
                        .working_directory()
                        .to_str()
                        .ok_or_else(|| error(ErrorCode::UnsupportedRepository))?
                        .into(),
                })?,
                EventKind::NativeScopeBound,
            ),
            Self::Drain(_) => {
                if run.state != RunState::Stopped {
                    return Err(error(ErrorCode::CorruptStore));
                }
                let sequence: i64 = c
                    .query_row(
                        "SELECT event_count FROM runs WHERE id=?1",
                        [run.binding.run_id.to_string()],
                        |row| row.get(0),
                    )
                    .db()?;
                (
                    encode(&NativeDrainRecord {
                        schema_version: SchemaVersion::V0_1_0,
                        scope: scope_record(c, task.task_id, run.binding.run_id)?,
                        observed_source_cursor: run.source_cursor,
                        execution_sequence: sequence as u64,
                    })?,
                    EventKind::NativeDrainSealed,
                )
            }
            Self::Ready(_) | Self::Tool(_) => return Err(error(ErrorCode::CorruptStore)),
        };
        append_event(
            c,
            r,
            &mut task,
            now,
            kind,
            None,
            Some(blake3::hash(json.as_bytes()).to_hex().as_ref()),
        )?;
        let sequence =
            i64::try_from(task.last_event_sequence).map_err(|_| error(ErrorCode::CorruptStore))?;
        match self {
            Self::Spawn(_, _) => c.execute(
                "INSERT INTO native_scopes VALUES (?1,?2,?3,?4,?5)",
                params![
                    run.binding.run_id.to_string(),
                    scope_id.to_string(),
                    task.task_id.to_string(),
                    sequence,
                    json
                ],
            ),
            Self::Drain(_) => c.execute(
                "INSERT INTO native_drains VALUES (?1,?2,?3,?4)",
                params![
                    run.binding.run_id.to_string(),
                    task.task_id.to_string(),
                    sequence,
                    json
                ],
            ),
            Self::Ready(_) | Self::Tool(_) => return Err(error(ErrorCode::CorruptStore)),
        }
        .db()?;
        Ok(())
    }
}

pub(super) fn tool_run(
    c: &Connection,
    root: &Path,
    binding: &RunBinding,
    proof: &NativeRunningProof<'_>,
) -> Result<(Task, RunRecord), Diagnostic> {
    access(c, binding.actor_id, binding.task_id)?;
    let task = load_task(c, binding.task_id)?;
    if task.scope != "." {
        return Err(error(ErrorCode::ExecutionNotEnabled));
    }
    if task.epoch != binding.epoch {
        return Err(error(ErrorCode::StaleEpoch));
    }
    if task.owner_id != Some(binding.actor_id) || task.lease_id != Some(binding.lease_id) {
        return Err(error(ErrorCode::Forbidden));
    }
    if !matches!(task.state, TaskState::Reserved | TaskState::Active)
        || transfers::pending(c, task.task_id)?
    {
        return Err(error(ErrorCode::Blocked));
    }
    let run = execution::load(c, task.task_id, binding.run_id)?;
    let scope = scope_record(c, task.task_id, binding.run_id)?;
    if run.binding != *binding
        || scope.binding != *binding
        || scope.scope_id != proof.scope_id()
        || scope.pid != proof.pid()
        || run.pid != Some(proof.pid())
        || scope.launch_fingerprint != proof.launch_fingerprint()
        || run.launch_fingerprint != proof.launch_fingerprint()
        || Path::new(&scope.launch_worktree) != proof.working_directory()
        || proof.working_directory() != snapshots::repository(c, root, &task)?
    {
        return Err(error(ErrorCode::Forbidden));
    }
    if run.state != RunState::Running || run.event_gap {
        return Err(error(ErrorCode::Blocked));
    }
    Ok((task, run))
}

pub(super) fn scope_record(
    c: &Connection,
    task: Id,
    run: Id,
) -> Result<NativeScopeRecord, Diagnostic> {
    let (json, sequence): (String, i64) = c
        .query_row(
            "SELECT record,sequence FROM native_scopes WHERE run_id=?1 AND task_id=?2",
            params![run.to_string(), task.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .db()?
        .ok_or_else(|| error(ErrorCode::NotFound))?;
    let event = verify_event(c, task, sequence, &json, EventKind::NativeScopeBound)?;
    let scope: NativeScopeRecord = decode(&json)?;
    if scope.binding.task_id != task
        || scope.binding.run_id != run
        || event.actor_id != scope.binding.actor_id
        || event.epoch != scope.binding.epoch
    {
        return Err(error(ErrorCode::CorruptStore));
    }
    Ok(scope)
}
fn verify_event(
    c: &Connection,
    task: Id,
    sequence: i64,
    json: &str,
    kind: EventKind,
) -> Result<Event, Diagnostic> {
    let event: String = c
        .query_row(
            "SELECT record FROM events WHERE task_id=?1 AND sequence=?2",
            params![task.to_string(), sequence],
            |row| row.get(0),
        )
        .optional()
        .db()?
        .ok_or_else(|| error(ErrorCode::CorruptStore))?;
    let event: Event = decode(&event)?;
    if event.kind != kind
        || event.task_id != task
        || u64::try_from(sequence).ok() != Some(event.sequence)
        || event.evidence_hash.as_deref() != Some(blake3::hash(json.as_bytes()).to_hex().as_str())
    {
        return Err(error(ErrorCode::CorruptStore));
    }
    Ok(event)
}

impl Store {
    /// Trusted adapter acknowledgement plus a sampled live scope. No process is spawned or replayed.
    pub fn record_receiver_ready(
        &mut self,
        actor: &Actor,
        transfer_id: Id,
        session: &ReceiverSession,
        proof: &NativeRunningProof<'_>,
        operation_id: Id,
    ) -> Result<TransferRecord, Diagnostic> {
        let request = Request {
            schema_version: SchemaVersion::V0_1_0,
            operation_id,
            actor_id: actor.id(),
            expected_epoch: session.binding.epoch,
            action: Action::AcknowledgeReceiver {
                task_id: session.binding.task_id,
                transfer_id,
                session: Box::new(session.clone()),
            },
        };
        match self
            .execute_context(actor, &request, false, Some(Evidence::Ready(proof)), None)?
            .0
        {
            Reply::Transfer(record) => Ok(record),
            _ => Err(error(ErrorCode::CorruptStore)),
        }
    }
    pub fn record_native_spawn(
        &mut self,
        actor: &Actor,
        permit: &LaunchPermit,
        tree: &ProcessTree,
        operation_id: Id,
    ) -> Result<RunRecord, Diagnostic> {
        self.native_observation(
            actor,
            &permit.record().binding,
            operation_id,
            RunObservation::Spawned { pid: tree.pid() },
            Evidence::Spawn(tree, permit),
        )
    }
    pub fn record_native_drain(
        &mut self,
        actor: &Actor,
        binding: &RunBinding,
        proof: &NativeDrainProof<'_>,
        operation_id: Id,
    ) -> Result<RunRecord, Diagnostic> {
        self.native_observation(
            actor,
            binding,
            operation_id,
            RunObservation::TreeStopped,
            Evidence::Drain(proof),
        )
    }
    fn native_observation(
        &mut self,
        actor: &Actor,
        binding: &RunBinding,
        operation_id: Id,
        observation: RunObservation,
        proof: Evidence<'_>,
    ) -> Result<RunRecord, Diagnostic> {
        let r = Request {
            schema_version: SchemaVersion::V0_1_0,
            operation_id,
            actor_id: actor.id(),
            expected_epoch: binding.epoch,
            action: Action::ObserveRun {
                task_id: binding.task_id,
                lease_id: binding.lease_id,
                run_id: binding.run_id,
                observation,
            },
        };
        match self.execute_context(actor, &r, false, Some(proof), None)?.0 {
            Reply::Run(run) => Ok(run),
            _ => Err(error(ErrorCode::CorruptStore)),
        }
    }
    /// Historical managed-tree drain fact, never complete safe-boundary authority.
    pub fn native_drain(
        &self,
        actor: &Actor,
        task: Id,
        run: Id,
    ) -> Result<NativeDrainRecord, Diagnostic> {
        let tx = self.connection.unchecked_transaction().db()?;
        read_task(&tx, actor, task)?;
        drain_record(&tx, task, run)
    }
}

pub(super) fn drain_record(
    c: &Connection,
    task: Id,
    run: Id,
) -> Result<NativeDrainRecord, Diagnostic> {
    let (json, sequence): (String, i64) = c
        .query_row(
            "SELECT record,sequence FROM native_drains WHERE run_id=?1 AND task_id=?2",
            params![run.to_string(), task.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .db()?
        .ok_or_else(|| error(ErrorCode::NotFound))?;
    let event = verify_event(c, task, sequence, &json, EventKind::NativeDrainSealed)?;
    let drain: NativeDrainRecord = decode(&json)?;
    let current = execution::load(c, task, run)?;
    let terminal: String = c
        .query_row(
            "SELECT record FROM execution_events WHERE run_id=?1 AND sequence=?2",
            params![
                run.to_string(),
                i64::try_from(drain.execution_sequence)
                    .map_err(|_| error(ErrorCode::CorruptStore))?
            ],
            |row| row.get(0),
        )
        .optional()
        .db()?
        .ok_or_else(|| error(ErrorCode::CorruptStore))?;
    let terminal: ExecutionEvent = decode(&terminal)?;
    if drain.scope != scope_record(c, task, run)?
        || current.binding != drain.scope.binding
        || current.state != RunState::Stopped
        || !current.tree_stopped
        || current.pid != Some(drain.scope.pid)
        || current.launch_fingerprint != drain.scope.launch_fingerprint
        || current.source_cursor != drain.observed_source_cursor
        || terminal.binding != drain.scope.binding
        || event.actor_id != drain.scope.binding.actor_id
        || event.epoch != drain.scope.binding.epoch
        || terminal.operation_id != event.operation_id
        || terminal.origin != RunObservation::TreeStopped.origin()
        || terminal.observation != RunObservation::TreeStopped
        || terminal.sequence != drain.execution_sequence
        || terminal.task_sequence.checked_add(1) != u64::try_from(sequence).ok()
    {
        return Err(error(ErrorCode::CorruptStore));
    }
    Ok(drain)
}
