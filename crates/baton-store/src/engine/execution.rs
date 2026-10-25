use super::*;
use baton_platform::process::LaunchSpec;

/// Fresh-only admission for a managed, bounded tool. No replay or external-effect permission.
pub struct ToolPermit {
    record: RunRecord,
    scope_id: Id,
}
impl ToolPermit {
    pub fn record(&self) -> &RunRecord {
        &self.record
    }
}

pub(super) struct ReceiverLaunch<'a> {
    run_id: Id,
    spec: &'a LaunchSpec,
    capabilities: AdapterCapabilities,
}
impl ReceiverLaunch<'_> {
    pub(super) fn fingerprint(&self, request: &Request) -> Result<String, Diagnostic> {
        let mut hash = blake3::Hasher::new();
        hash.update(b"receiver-launch-v1:");
        hash.update(request_fingerprint(request, false)?.as_bytes());
        hash.update(self.run_id.as_bytes());
        hash.update(self.spec.fingerprint().as_bytes());
        hash.update(encode(&self.capabilities)?.as_bytes());
        Ok(hash.finalize().to_hex().to_string())
    }
    pub(super) fn reserve(
        &self,
        c: &Connection,
        root: &Path,
        r: &Request,
        task: &mut Task,
        now: i64,
        clock: &dyn Clock,
    ) -> Result<Reply, Diagnostic> {
        let Action::ActivateReceiver { transfer_id, .. } = r.action else {
            return Err(error(ErrorCode::InvalidInput));
        };
        let mut transfer = transfers::record(c, task.task_id, transfer_id)?;
        if r.actor_id != transfer.binding.receiver_id || task.owner_id != Some(r.actor_id) {
            return Err(error(ErrorCode::Forbidden));
        }
        if task.state != TaskState::Committed
            || transfer.state != TransferState::Committed
            || transfer.committed_epoch != Some(task.epoch)
            || self.run_id.is_nil()
        {
            return Err(error(ErrorCode::Blocked));
        }
        if c.prepare("SELECT 1 FROM runs WHERE id=?1 OR (task_id=?2 AND json_extract(record,'$.binding.epoch')=?3)").db()?
            .exists(params![self.run_id.to_string(), task.task_id.to_string(), i64::try_from(task.epoch).map_err(|_| error(ErrorCode::CorruptStore))?]).db()?
        {
            return Err(error(ErrorCode::Blocked));
        }
        let facts = transfers::facts(c, root, task, &transfer)?;
        if !facts.missing(true).is_empty()
            || !self.capabilities.lifecycle
            || !self.capabilities.tool_outcomes
            || !self.capabilities.interrupt
            || !self.capabilities.mutation_gating
        {
            return Err(error(ErrorCode::Blocked));
        }
        let report = transfer
            .receiver_stage
            .as_ref()
            .and_then(|s| s.report.as_ref())
            .ok_or_else(|| error(ErrorCode::CorruptStore))?;
        if self.spec.working_directory()
            != fs::canonicalize(&report.worktree).map_err(|_| error(ErrorCode::Io))?
        {
            return Err(error(ErrorCode::Forbidden));
        }
        snapshots::finish(c, r, task, now, clock)?;
        let run = RunRecord::pending(
            RunBinding {
                run_id: self.run_id,
                task_id: task.task_id,
                actor_id: r.actor_id,
                lease_id: task
                    .lease_id
                    .ok_or_else(|| error(ErrorCode::CorruptStore))?,
                epoch: task.epoch,
            },
            self.spec.fingerprint(),
            self.capabilities.clone(),
        );
        transfer.advance(r.actor_id, task.epoch, TransferStep::Activate, &facts)?;
        transfer.receiver_activation = Some(Box::new(run.clone()));
        c.execute(
            "INSERT INTO runs VALUES (?1,?2,?3,0)",
            params![
                self.run_id.to_string(),
                task.task_id.to_string(),
                encode(&run)?
            ],
        )
        .db()?;
        task.state = TaskState::Activating;
        transfers::persist(
            c,
            r,
            task,
            now,
            transfer,
            EventKind::ReceiverActivationReserved,
        )?;
        Ok(Reply::Run(run))
    }
}

/// One-use, in-memory launch permission, issued only after a fresh durable intent.
/// No Clone/Deserialize: a repeated request or restarted caller cannot respawn.
pub struct LaunchPermit {
    record: RunRecord,
    transfer: Option<Id>,
}
impl LaunchPermit {
    pub fn record(&self) -> &RunRecord {
        &self.record
    }
    pub fn transfer_id(&self) -> Option<Id> {
        self.transfer
    }
    pub(super) fn matches_grant(&self, c: &Connection) -> Result<bool, Diagnostic> {
        if let Some(id) = self.transfer {
            let transfer = transfers::record(c, self.record.binding.task_id, id)?;
            Ok(transfer.state == TransferState::Activating
                && transfer.receiver_activation.as_deref() == Some(&self.record))
        } else {
            Ok(!transfers::pending(c, self.record.binding.task_id)?)
        }
    }
}

impl Store {
    pub fn execution_event(
        &self,
        actor: &Actor,
        task: Id,
        run: Id,
        operation: Id,
    ) -> Result<Option<ExecutionEvent>, Diagnostic> {
        let tx = self.connection.unchecked_transaction().db()?;
        read_task(&tx, actor, task)?;
        let record = load(&tx, task, run)?;
        let json:Option<String>=tx.query_row("SELECT record FROM execution_events WHERE run_id=?1 AND json_extract(record,'$.operation_id')=?2",
            params![run.to_string(),operation.to_string()],|row|row.get(0)).optional().db()?;
        json.map(|json| {
            let event: ExecutionEvent = decode(&json)?;
            if event.binding != record.binding || event.operation_id != operation {
                return Err(error(ErrorCode::CorruptStore));
            }
            verify_adapter_event(&tx, &event)?;
            Ok(event)
        })
        .transpose()
    }
    pub fn prepare_tool(
        &mut self,
        actor: &Actor,
        request: &Request,
        proof: &baton_platform::process::NativeRunningProof<'_>,
    ) -> Result<(RunRecord, Option<ToolPermit>), Diagnostic> {
        if !matches!(
            request.action,
            Action::ObserveRun {
                observation: RunObservation::ToolReserved { .. },
                ..
            }
        ) {
            return Err(error(ErrorCode::InvalidInput));
        }
        let (reply, fresh) = self.execute_context(
            actor,
            request,
            false,
            Some(native::Evidence::Tool(proof)),
            None,
        )?;
        match reply {
            Reply::Run(record) => {
                let permit = fresh.then(|| ToolPermit {
                    record: record.clone(),
                    scope_id: proof.scope_id(),
                });
                Ok((record, permit))
            }
            _ => Err(error(ErrorCode::CorruptStore)),
        }
    }
    pub fn consume_tool(
        &self,
        actor: &Actor,
        permit: ToolPermit,
        proof: &baton_platform::process::NativeRunningProof<'_>,
    ) -> Result<(), Diagnostic> {
        authenticate(&self.connection, actor)?;
        if actor.id() != permit.record.binding.actor_id {
            return Err(error(ErrorCode::Forbidden));
        }
        let (task, run) =
            native::tool_run(&self.connection, &self.root, &permit.record.binding, proof)?;
        let high: i64 = self
            .connection
            .query_row("SELECT last_ms FROM clock WHERE singleton=1", [], |row| {
                row.get(0)
            })
            .db()?;
        let now = self.clock.now_ms()?;
        if now < 0
            || task
                .expires_at_ms
                .is_none_or(|expiry| now.max(high) >= expiry)
            || permit.scope_id != proof.scope_id()
            || run != permit.record
        {
            return Err(error(ErrorCode::Blocked));
        }
        Ok(())
    }
    /// Trusted adapter embedding only. Neither launch input nor the fresh permit is replayable over IPC.
    pub fn prepare_receiver_launch(
        &mut self,
        actor: &Actor,
        request: &Request,
        run_id: Id,
        spec: &LaunchSpec,
        capabilities: AdapterCapabilities,
    ) -> Result<(RunRecord, Option<LaunchPermit>), Diagnostic> {
        let Action::ActivateReceiver { transfer_id, .. } = request.action else {
            return Err(error(ErrorCode::InvalidInput));
        };
        let launch = ReceiverLaunch {
            run_id,
            spec,
            capabilities,
        };
        let (reply, fresh) = self.execute_context(actor, request, false, None, Some(&launch))?;
        match reply {
            Reply::Run(record) => {
                let permit = fresh.then(|| LaunchPermit {
                    record: record.clone(),
                    transfer: Some(transfer_id),
                });
                Ok((record, permit))
            }
            _ => Err(error(ErrorCode::CorruptStore)),
        }
    }
    pub fn prepare_launch(
        &mut self,
        actor: &Actor,
        request: &Request,
    ) -> Result<(RunRecord, Option<LaunchPermit>), Diagnostic> {
        if !matches!(request.action, Action::OpenRun { .. }) {
            return Err(error(ErrorCode::InvalidInput));
        }
        let (reply, fresh) = self.execute_fresh(actor, request, false)?;
        match reply {
            Reply::Run(record) => {
                let permit = fresh.then(|| LaunchPermit {
                    record: record.clone(),
                    transfer: None,
                });
                Ok((record, permit))
            }
            _ => Err(error(ErrorCode::CorruptStore)),
        }
    }

    pub fn validate_launch(&self, actor: &Actor, permit: &LaunchPermit) -> Result<(), Diagnostic> {
        let record = permit.record();
        if !permit.matches_grant(&self.connection)? {
            return Err(error(ErrorCode::Blocked));
        }
        let task = self.task(actor, record.binding.task_id)?;
        let high: i64 = self
            .connection
            .query_row("SELECT last_ms FROM clock WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .db()?;
        let now = self.clock.now_ms()?;
        if now < 0
            || task.epoch != record.binding.epoch
            || task.owner_id != Some(actor.id())
            || task.lease_id != Some(record.binding.lease_id)
            || task.state
                != if permit.transfer.is_some() {
                    TaskState::Activating
                } else {
                    TaskState::Reserved
                }
            || task
                .expires_at_ms
                .is_none_or(|expiry| now.max(high) >= expiry)
            || self.run(actor, task.task_id, record.binding.run_id)? != *record
        {
            return Err(error(ErrorCode::Blocked));
        }
        if let Some(id) = permit.transfer {
            let transfer = transfers::record(&self.connection, task.task_id, id)?;
            if !transfers::facts(&self.connection, &self.root, &task, &transfer)?
                .missing(true)
                .is_empty()
            {
                return Err(error(ErrorCode::Blocked));
            }
        }
        let end = self.clock.now_ms()?;
        if end < 0
            || task
                .expires_at_ms
                .is_none_or(|expiry| end.max(now).max(high) >= expiry)
        {
            return Err(error(ErrorCode::Blocked));
        }
        Ok(())
    }

    pub fn run(&self, actor: &Actor, task: Id, run: Id) -> Result<RunRecord, Diagnostic> {
        let tx = self.connection.unchecked_transaction().db()?;
        read_task(&tx, actor, task)?;
        load(&tx, task, run)
    }

    pub fn execution_events(
        &self,
        actor: &Actor,
        task: Id,
        run: Id,
    ) -> Result<Vec<ExecutionEvent>, Diagnostic> {
        let tx = self.connection.unchecked_transaction().db()?;
        read_task(&tx, actor, task)?;
        let record = load(&tx, task, run)?;
        let mut statement = tx
            .prepare("SELECT record FROM execution_events WHERE run_id=?1 ORDER BY sequence")
            .db()?;
        let mut events = Vec::new();
        for row in statement
            .query_map([run.to_string()], |r| r.get::<_, String>(0))
            .db()?
        {
            let event: ExecutionEvent = decode(&row.db()?)?;
            if event.binding != record.binding || event.sequence != events.len() as u64 + 1 {
                return Err(error(ErrorCode::CorruptStore));
            }
            verify_adapter_event(&tx, &event)?;
            events.push(event);
        }
        let count: i64 = tx
            .query_row(
                "SELECT event_count FROM runs WHERE id=?1",
                [run.to_string()],
                |r| r.get(0),
            )
            .db()?;
        if u64::try_from(count).ok() != Some(events.len() as u64) {
            return Err(error(ErrorCode::CorruptStore));
        }
        Ok(events)
    }
}

pub(super) fn load(c: &Connection, task: Id, run: Id) -> Result<RunRecord, Diagnostic> {
    let json: String = c
        .query_row(
            "SELECT record FROM runs WHERE id=?1 AND task_id=?2",
            params![run.to_string(), task.to_string()],
            |r| r.get(0),
        )
        .optional()
        .db()?
        .ok_or_else(|| error(ErrorCode::NotFound))?;
    let record: RunRecord = decode(&json)?;
    if record.binding.task_id != task || record.binding.run_id != run {
        return Err(error(ErrorCode::CorruptStore));
    }
    let expected: i64 = c
        .query_row(
            "SELECT event_count FROM runs WHERE id=?1",
            [run.to_string()],
            |r| r.get(0),
        )
        .db()?;
    let (count, last): (i64, i64) = c
        .query_row(
            "SELECT count(*),coalesce(max(sequence),0) FROM execution_events WHERE run_id=?1",
            [run.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .db()?;
    if expected != count || count != last {
        return Err(error(ErrorCode::CorruptStore));
    }
    adapter_history(c, &record.binding)?;
    Ok(record)
}

fn verify_adapter_event(c: &Connection, event: &ExecutionEvent) -> Result<(), Diagnostic> {
    if !matches!(
        event.observation,
        RunObservation::AdapterReported { .. } | RunObservation::AdapterGap { .. }
    ) {
        return Ok(());
    }
    let json: String = c
        .query_row(
            "SELECT record FROM events WHERE task_id=?1 AND sequence=?2",
            params![
                event.binding.task_id.to_string(),
                i64::try_from(event.task_sequence).map_err(|_| error(ErrorCode::CorruptStore))?
            ],
            |row| row.get(0),
        )
        .optional()
        .db()?
        .ok_or_else(|| error(ErrorCode::CorruptStore))?;
    let task_event: Event = decode(&json)?;
    if task_event.kind != EventKind::ExecutionObserved
        || task_event.task_id != event.binding.task_id
        || task_event.sequence != event.task_sequence
        || event.origin != event.observation.origin()
        || task_event.actor_id != event.binding.actor_id
        || task_event.epoch != event.binding.epoch
        || task_event.operation_id != event.operation_id
        || task_event.timestamp_ms != event.timestamp_ms
        || task_event.evidence_hash.as_deref()
            != Some(blake3::hash(encode(event)?.as_bytes()).to_hex().as_str())
    {
        return Err(error(ErrorCode::CorruptStore));
    }
    Ok(())
}
pub(super) fn adapter_history(
    c: &Connection,
    binding: &RunBinding,
) -> Result<Vec<AdapterEvent>, Diagnostic> {
    let mut statement = c
        .prepare("SELECT record FROM execution_events WHERE run_id=?1 ORDER BY sequence")
        .db()?;
    let mut history = vec![];
    for row in statement
        .query_map([binding.run_id.to_string()], |row| row.get::<_, String>(0))
        .db()?
    {
        let event: ExecutionEvent = decode(&row.db()?)?;
        verify_adapter_event(c, &event)?;
        if let RunObservation::AdapterReported {
            event: ref adapter, ..
        } = event.observation
        {
            if &event.binding != binding {
                return Err(error(ErrorCode::CorruptStore));
            }
            adapter
                .validate()
                .map_err(|_| error(ErrorCode::CorruptStore))?;
            if history.first().is_some_and(|first: &AdapterEvent| {
                first.adapter != adapter.adapter || first.thread_id != adapter.thread_id
            }) {
                return Err(error(ErrorCode::CorruptStore));
            }
            history.push(adapter.clone());
        }
    }
    Ok(history)
}

pub(super) fn apply(
    c: &Connection,
    r: &Request,
    task: &mut Task,
    now: i64,
) -> Result<Reply, Diagnostic> {
    match &r.action {
        Action::OpenRun {
            run_id,
            lease_id,
            launch_fingerprint,
            capabilities,
            ..
        } => {
            if run_id.is_nil() || !valid_evidence_hash(launch_fingerprint) {
                return Err(error(ErrorCode::InvalidInput));
            }
            if c.prepare("SELECT 1 FROM runs WHERE id=?1 OR task_id=?2")
                .db()?
                .exists(params![run_id.to_string(), task.task_id.to_string()])
                .db()?
            {
                return Err(error(ErrorCode::Blocked));
            }
            let record = RunRecord::pending(
                RunBinding {
                    run_id: *run_id,
                    task_id: task.task_id,
                    actor_id: r.actor_id,
                    lease_id: *lease_id,
                    epoch: task.epoch,
                },
                launch_fingerprint.clone(),
                capabilities.clone(),
            );
            c.execute(
                "INSERT INTO runs VALUES (?1,?2,?3,0)",
                params![
                    run_id.to_string(),
                    task.task_id.to_string(),
                    encode(&record)?
                ],
            )
            .db()?;
            append_event(c, r, task, now, EventKind::RunReserved, None, None)?;
            Ok(Reply::Run(record))
        }
        Action::ObserveRun {
            run_id,
            observation,
            ..
        } => {
            let mut record = load(c, task.task_id, *run_id)?;
            if record.binding.epoch != r.expected_epoch || record.binding.actor_id != r.actor_id {
                return Err(error(ErrorCode::StaleEpoch));
            }
            let count: i64 = c
                .query_row(
                    "SELECT event_count FROM runs WHERE id=?1",
                    [run_id.to_string()],
                    |r| r.get(0),
                )
                .db()?;
            let actual: i64 = c
                .query_row(
                    "SELECT count(*) FROM execution_events WHERE run_id=?1",
                    [run_id.to_string()],
                    |r| r.get(0),
                )
                .db()?;
            if count != actual {
                return Err(error(ErrorCode::CorruptStore));
            }
            let sequence = count
                .checked_add(1)
                .filter(|n| *n <= 100_000)
                .ok_or_else(|| error(ErrorCode::Blocked))?;
            if let RunObservation::AdapterReported { event, .. } = observation {
                record.event_gap |= event.follows(&adapter_history(c, &record.binding)?)?;
            }
            record.apply(observation)?;
            let event = ExecutionEvent {
                schema_version: SchemaVersion::V0_1_0,
                event_id: Id::new_v4(),
                binding: record.binding.clone(),
                sequence: sequence as u64,
                task_sequence: task
                    .last_event_sequence
                    .checked_add(1)
                    .ok_or_else(|| error(ErrorCode::Blocked))?,
                operation_id: r.operation_id,
                timestamp_ms: now,
                origin: observation.origin(),
                evidence_hash: None,
                observation: observation.clone(),
            };
            let evidence = matches!(
                observation,
                RunObservation::AdapterReported { .. } | RunObservation::AdapterGap { .. }
            )
            .then(|| encode(&event).map(|json| blake3::hash(json.as_bytes()).to_hex().to_string()))
            .transpose()?;
            append_event(
                c,
                r,
                task,
                now,
                EventKind::ExecutionObserved,
                None,
                evidence.as_deref(),
            )?;
            c.execute(
                "INSERT INTO execution_events VALUES (?1,?2,?3)",
                params![run_id.to_string(), sequence, encode(&event)?],
            )
            .db()?;
            c.execute(
                "UPDATE runs SET record=?1,event_count=?2 WHERE id=?3",
                params![encode(&record)?, sequence, run_id.to_string()],
            )
            .db()?;
            Ok(Reply::Run(record))
        }
        _ => Err(error(ErrorCode::InvalidInput)),
    }
}
