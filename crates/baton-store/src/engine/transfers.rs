use super::*;

#[cfg(test)]
std::thread_local! { static SIMULATED_BOUNDARY: std::cell::Cell<Option<Id>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(super) fn simulate_boundary(task: Id) {
    SIMULATED_BOUNDARY.with(|scope| scope.set(Some(task)));
}

pub(super) fn owns_restore(c: &Connection, restore: Id) -> Result<bool, Diagnostic> {
    c.prepare("SELECT 1 FROM transfers WHERE json_extract(record,'$.restore_id')=?1")
        .db()?
        .exists([restore.to_string()])
        .db()
}
fn checkpoint(
    c: &Connection,
    root: &Path,
    transfer: &TransferRecord,
) -> Result<CheckpointRecord, Diagnostic> {
    let checkpoint = transfer
        .checkpoint
        .as_ref()
        .ok_or_else(|| error(ErrorCode::Blocked))?;
    let actual = snapshots::record(c, root, transfer.binding.task_id, checkpoint.checkpoint_id)?;
    if actual.manifest_hash != checkpoint.manifest_hash
        || actual.binding.epoch != transfer.binding.source_epoch
        || actual.binding.actor_id != transfer.binding.source_id
    {
        return Err(error(ErrorCode::IntegrityFailure));
    }
    Ok(actual)
}
pub(super) fn facts(
    c: &Connection,
    root: &Path,
    task: &Task,
    transfer: &TransferRecord,
) -> Result<TransferFacts, Diagnostic> {
    checked_facts(c, root, task, transfer, true)
}
fn checked_facts(
    c: &Connection,
    root: &Path,
    task: &Task,
    transfer: &TransferRecord,
    validate_receiver: bool,
) -> Result<TransferFacts, Diagnostic> {
    let checkpoint = checkpoint(c, root, transfer)?;
    let report = transfer
        .receiver_stage
        .as_ref()
        .and_then(|stage| stage.report.as_ref());
    if let Some(report) = report {
        if Some(report.restore_id) != transfer.restore_id {
            return Err(error(ErrorCode::CorruptStore));
        }
        if validate_receiver {
            snapshots::vault(root)?.verify_restore(
                &snapshots::repository(c, root, task)?,
                &checkpoint.manifest_hash,
                report,
            )?;
        }
    }
    let facts = TransferFacts {
        artifacts_verified: !checkpoint.reasons.contains(&CheckpointReason::CaptureGaps),
        artifacts_durable: !checkpoint
            .reasons
            .contains(&CheckpointReason::DurabilityUnproven),
        receiver_verified: report.is_some(),
        ..TransferFacts::default()
    };
    // Fixture assumptions exercise SQL/file transitions; no release build includes this verifier.
    #[cfg(test)]
    let facts = if SIMULATED_BOUNDARY.with(|scope| scope.get() == Some(task.task_id)) {
        TransferFacts {
            source_fenced: true,
            events_complete: true,
            effects_settled: true,
            adapter_validated: true,
            source_isolated: true,
            artifacts_durable: true,
            ..facts
        }
    } else {
        facts
    };
    let mut facts = facts;
    if task.state == TaskState::Quiescing && transfer.source_drain.is_none() {
        facts.source_fenced = false;
    }
    if let Some(drain) = &transfer.source_drain {
        let run = execution::load(c, task.task_id, drain.scope.binding.run_id)?;
        let blockers = run.blockers();
        facts.events_complete &= !blockers.contains(&BoundaryReason::EventGap);
        facts.effects_settled &= !blockers.contains(&BoundaryReason::UnresolvedEffect);
        facts.adapter_validated &= !blockers.contains(&BoundaryReason::AdapterUnsupported)
            && !blockers.contains(&BoundaryReason::MutationGatingUnavailable);
    }
    Ok(facts)
}

fn holds_slot(record: &TransferRecord) -> bool {
    !matches!(
        record.state,
        TransferState::Active | TransferState::FailedPrecommit
    ) && !(record.state == TransferState::RecoveryRequired && record.committed_epoch.is_some())
}

pub(super) fn grant(c: &Connection, task: &Task) -> Result<Option<TransferRecord>, Diagnostic> {
    let id: Option<String> = c.query_row("SELECT id FROM transfers WHERE task_id=?1 AND json_extract(record,'$.committed_epoch')=?2",
        params![task.task_id.to_string(), i64::try_from(task.epoch).map_err(|_| error(ErrorCode::CorruptStore))?], |row| row.get(0)).optional().db()?;
    id.map(|id| {
        let record = record(
            c,
            task.task_id,
            Id::parse_str(&id).map_err(|_| error(ErrorCode::CorruptStore))?,
        )?;
        if task.owner_id != Some(record.binding.receiver_id) {
            return Err(error(ErrorCode::CorruptStore));
        }
        Ok(record)
    })
    .transpose()
}

pub(super) fn pending(c: &Connection, task: Id) -> Result<bool, Diagnostic> {
    let mut statement = c
        .prepare("SELECT id FROM transfers WHERE task_id=?1")
        .db()?;
    let mut pending = false;
    for id in statement
        .query_map([task.to_string()], |r| r.get::<_, String>(0))
        .db()?
    {
        let id = Id::parse_str(&id.db()?).map_err(|_| error(ErrorCode::CorruptStore))?;
        pending |= holds_slot(&record(c, task, id)?);
    }
    Ok(pending)
}

pub(super) fn record(c: &Connection, task: Id, transfer: Id) -> Result<TransferRecord, Diagnostic> {
    let (json, active): (String, bool) = c
        .query_row(
            "SELECT record,active FROM transfers WHERE id=?1 AND task_id=?2",
            params![transfer.to_string(), task.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .db()?
        .ok_or_else(|| error(ErrorCode::NotFound))?;
    let record: TransferRecord = decode(&json)?;
    let (latest, sequence): (String, i64) = c.query_row(
        "SELECT record,sequence FROM transfer_events WHERE transfer_id=?1 ORDER BY sequence DESC LIMIT 1",
        [transfer.to_string()], |r| Ok((r.get(0)?, r.get(1)?)),
    ).db()?;
    let event: String = c
        .query_row(
            "SELECT record FROM events WHERE task_id=?1 AND sequence=?2",
            params![task.to_string(), sequence],
            |r| r.get(0),
        )
        .db()?;
    let event: Event = decode(&event)?;
    if latest != json
        || record.binding.task_id != task
        || record.binding.transfer_id != transfer
        || record.binding.source_epoch == 0
        || record.binding.source_epoch >= i64::MAX as u64
        || active != holds_slot(&record)
        || record
            .committed_epoch
            .is_some_and(|epoch| epoch != record.binding.source_epoch + 1)
        || (record.state == TransferState::Committed && record.committed_epoch.is_none())
        || (record.committed_epoch.is_some()
            && !matches!(
                record.state,
                TransferState::Committed
                    | TransferState::Activating
                    | TransferState::Active
                    | TransferState::RecoveryRequired
            ))
        || record.quality != RecoveryQuality::Blocked
        || (record.state == TransferState::Activating && record.receiver_activation.is_none())
        || (record.state == TransferState::Active && record.receiver_ready.is_none())
        || (record.receiver_ready.is_some()
            && !matches!(
                record.state,
                TransferState::Active | TransferState::RecoveryRequired
            ))
        || record.receiver_activation.as_ref().is_some_and(|run| {
            run.binding.task_id != task
                || run.binding.actor_id != record.binding.receiver_id
                || Some(run.binding.epoch) != record.committed_epoch
        })
        || event.evidence_hash.as_deref() != Some(blake3::hash(json.as_bytes()).to_hex().as_str())
    {
        return Err(error(ErrorCode::CorruptStore));
    }
    if let Some(activation) = &record.receiver_activation {
        let run = execution::load(c, task, activation.binding.run_id)?;
        if run.binding != activation.binding
            || run.launch_fingerprint != activation.launch_fingerprint
            || run.capabilities != activation.capabilities
        {
            return Err(error(ErrorCode::CorruptStore));
        }
    }
    if let Some(ready) = &record.receiver_ready {
        let activation = record
            .receiver_activation
            .as_ref()
            .ok_or_else(|| error(ErrorCode::CorruptStore))?;
        if ready.scope != native::scope_record(c, task, activation.binding.run_id)?
            || ready.session.binding != activation.binding
        {
            return Err(error(ErrorCode::CorruptStore));
        }
    }
    if let Some(drain) = &record.source_drain {
        if native::drain_record(c, task, drain.scope.binding.run_id)? != *drain
            || drain.scope.binding.epoch != record.binding.source_epoch
            || drain.scope.binding.actor_id != record.binding.source_id
        {
            return Err(error(ErrorCode::CorruptStore));
        }
    }
    Ok(record)
}

pub(super) fn apply(
    c: &Connection,
    root: &Path,
    r: &Request,
    task: &mut Task,
    now: i64,
    local: bool,
    clock: &dyn Clock,
) -> Result<Reply, Diagnostic> {
    let (record, kind) = match r.action {
        Action::AcknowledgeReceiver {
            transfer_id,
            ref session,
            ..
        } => {
            let mut record = record(c, task.task_id, transfer_id)?;
            if r.actor_id != record.binding.receiver_id
                || session.binding.actor_id != r.actor_id
                || task.owner_id != Some(r.actor_id)
                || task.lease_id != Some(session.binding.lease_id)
            {
                return Err(error(ErrorCode::Forbidden));
            }
            if task.state != TaskState::Activating
                || record.state != TransferState::Activating
                || record.committed_epoch != Some(task.epoch)
                || record
                    .receiver_activation
                    .as_ref()
                    .is_none_or(|run| run.binding != session.binding)
            {
                return Err(error(ErrorCode::Blocked));
            }
            if session.session_id.is_nil() {
                return Err(error(ErrorCode::InvalidInput));
            }
            if c.prepare("SELECT 1 FROM transfers WHERE json_extract(record,'$.receiver_ready.session.session_id')=?1").db()?.exists([session.session_id.to_string()]).db()? {
                return Err(error(ErrorCode::Blocked));
            }
            text(&session.adapter.name, 128)?;
            text(&session.adapter.version, 128)?;
            let mut facts = checked_facts(c, root, task, &record, false)?;
            if !facts.missing(true).is_empty() {
                return Err(error(ErrorCode::Blocked));
            }
            snapshots::finish(c, r, task, now, clock)?;
            facts.receiver_started = true;
            record.advance(
                r.actor_id,
                task.epoch,
                TransferStep::ReceiverStarted,
                &facts,
            )?;
            if record.state != TransferState::Active {
                return Err(error(ErrorCode::Blocked));
            }
            record.receiver_ready = Some(Box::new(ReceiverReadyRecord {
                schema_version: SchemaVersion::V0_1_0,
                scope: native::scope_record(c, task.task_id, session.binding.run_id)?,
                session: *session.clone(),
            }));
            // Generic readiness is not validation of a real vendor adapter/session.
            record.quality = RecoveryQuality::Blocked;
            record.reasons = vec![TransferReason::ReceiverAdapterUnvalidated];
            if record.optional_context_missing {
                record.reasons.push(TransferReason::OptionalContextMissing);
            }
            task.state = TaskState::Active;
            (record, EventKind::ReceiverReady)
        }
        Action::RequestTransfer {
            transfer_id,
            receiver_id,
            ..
        } => {
            if !matches!(task.state, TaskState::Reserved | TaskState::Active) {
                return Err(error(ErrorCode::Blocked));
            }
            if pending(c, task.task_id)? {
                return Err(error(ErrorCode::Blocked));
            }
            access(c, receiver_id, task.task_id)?;
            if c.prepare("SELECT 1 FROM transfers WHERE id=?1")
                .db()?
                .exists([transfer_id.to_string()])
                .db()?
            {
                return Err(error(ErrorCode::InvalidInput));
            }
            let record = TransferRecord::requested(TransferBinding {
                transfer_id,
                task_id: task.task_id,
                source_id: r.actor_id,
                receiver_id,
                source_epoch: task.epoch,
            })?;
            c.execute(
                "INSERT INTO transfers VALUES (?1,?2,1,?3)",
                params![
                    transfer_id.to_string(),
                    task.task_id.to_string(),
                    encode(&record)?
                ],
            )
            .db()?;
            let managed = c.prepare("SELECT 1 FROM runs WHERE task_id=?1 AND json_extract(record,'$.binding.epoch')=?2 AND json_extract(record,'$.state')!='LAUNCH_PENDING'").db()?
                .exists(params![task.task_id.to_string(), i64::try_from(task.epoch).map_err(|_| error(ErrorCode::CorruptStore))?]).db()?;
            if task.state == TaskState::Active || managed {
                task.state = TaskState::Quiescing;
            }
            (record, EventKind::TransferRequested)
        }
        Action::PreflightTransfer {
            transfer_id,
            checkpoint_id,
            ..
        } => {
            let mut record = record(c, task.task_id, transfer_id)?;
            let checkpoint = snapshots::record(c, root, task.task_id, checkpoint_id)?;
            if checkpoint.binding.epoch != task.epoch || checkpoint.binding.actor_id != r.actor_id {
                return Err(error(ErrorCode::StaleEpoch));
            }
            // No production fence/event-seal/adapter proof exists yet. Stored claims cannot supply it.
            let native_run: Option<String> = c
                .query_row(
                    "SELECT run_id FROM native_drains WHERE task_id=?1 AND json_extract(record,'$.scope.binding.epoch')=?2 AND json_extract(record,'$.scope.binding.actor_id')=?3",
                    params![task.task_id.to_string(), i64::try_from(task.epoch).map_err(|_| error(ErrorCode::CorruptStore))?, r.actor_id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .db()?;
            record.source_drain = native_run
                .map(|run| {
                    let run = Id::parse_str(&run).map_err(|_| error(ErrorCode::CorruptStore))?;
                    let drain = native::drain_record(c, task.task_id, run)?;
                    if drain.scope.binding.epoch != task.epoch
                        || drain.scope.binding.actor_id != r.actor_id
                    {
                        return Err(error(ErrorCode::StaleEpoch));
                    }
                    Ok(drain)
                })
                .transpose()?;
            record.checkpoint = Some(TransferCheckpoint {
                checkpoint_id,
                manifest_hash: checkpoint.manifest_hash.clone(),
                event_cursor: checkpoint.binding.event_cursor,
            });
            let facts = facts(c, root, task, &record)?;
            record.advance(
                r.actor_id,
                task.epoch,
                TransferStep::Prepare(TransferCheckpoint {
                    checkpoint_id,
                    manifest_hash: checkpoint.manifest_hash,
                    event_cursor: checkpoint.binding.event_cursor,
                }),
                &facts,
            )?;
            (record, EventKind::TransferPreflight)
        }
        Action::BeginReceiverStaging {
            transfer_id,
            restore_id,
            stage_operation_id,
            ..
        } => {
            let mut record = record(c, task.task_id, transfer_id)?;
            if r.actor_id != record.binding.receiver_id {
                return Err(error(ErrorCode::Forbidden));
            }
            if record.state != TransferState::Prepared
                || !matches!(task.state, TaskState::Reserved | TaskState::Quiescing)
            {
                return Err(error(ErrorCode::Blocked));
            }
            if restore_id.is_nil()
                || stage_operation_id.is_nil()
                || stage_operation_id == r.operation_id
            {
                return Err(error(ErrorCode::InvalidInput));
            }
            if owns_restore(c, restore_id)? || c.prepare("SELECT 1 FROM restore_intents WHERE restore_id=?1 OR operation_id=?2 UNION ALL SELECT 1 FROM operations WHERE id=?2 UNION ALL SELECT 1 FROM transfers WHERE json_extract(record,'$.receiver_stage.operation_id')=?2")
                .db()?.exists(params![restore_id.to_string(), stage_operation_id.to_string()]).db()? { return Err(error(ErrorCode::IdempotencyConflict)); }
            let facts = facts(c, root, task, &record)?;
            record.advance(
                r.actor_id,
                r.expected_epoch,
                TransferStep::BeginRestore,
                &facts,
            )?;
            if record.state == TransferState::Restoring {
                let stage = Request {
                    schema_version: SchemaVersion::V0_1_0,
                    operation_id: stage_operation_id,
                    actor_id: r.actor_id,
                    expected_epoch: r.expected_epoch,
                    action: Action::StageReceiver {
                        task_id: task.task_id,
                        transfer_id,
                    },
                };
                record.restore_id = Some(restore_id);
                record.receiver_stage = Some(Box::new(ReceiverStage {
                    operation_id: stage_operation_id,
                    fingerprint: request_fingerprint(&stage, local)?,
                    report: None,
                }));
            }
            let kind = if record.state == TransferState::Restoring {
                EventKind::ReceiverStagingReserved
            } else {
                EventKind::ReceiverStageChecked
            };
            (record, kind)
        }
        Action::StageReceiver { transfer_id, .. } => {
            let mut record = record(c, task.task_id, transfer_id)?;
            if r.actor_id != record.binding.receiver_id {
                return Err(error(ErrorCode::Forbidden));
            }
            if record.state != TransferState::Restoring
                || !matches!(task.state, TaskState::Reserved | TaskState::Quiescing)
            {
                return Err(error(ErrorCode::Blocked));
            }
            let fingerprint = request_fingerprint(r, local)?;
            let stage = record
                .receiver_stage
                .as_ref()
                .ok_or_else(|| error(ErrorCode::CorruptStore))?;
            if stage.operation_id != r.operation_id || stage.fingerprint != fingerprint {
                return Err(error(ErrorCode::IdempotencyConflict));
            }
            let checked = facts(c, root, task, &record)?;
            let missing = checked.missing(false);
            if !missing.is_empty() {
                record.state = TransferState::Blocked;
                record.reasons = missing;
            } else {
                let checkpoint = checkpoint(c, root, &record)?;
                let restore_id = record
                    .restore_id
                    .ok_or_else(|| error(ErrorCode::CorruptStore))?;
                let report = snapshots::vault(root)?.restore_recoverable(
                    &snapshots::repository(c, root, task)?,
                    &checkpoint.manifest_hash,
                    restore_id,
                    &fingerprint,
                )?;
                snapshots::finish(c, r, task, now, clock)?;
                record
                    .receiver_stage
                    .as_mut()
                    .ok_or_else(|| error(ErrorCode::CorruptStore))?
                    .report = Some(report);
                let checked = facts(c, root, task, &record)?;
                record.advance(
                    r.actor_id,
                    r.expected_epoch,
                    TransferStep::ReceiverValidated { restore_id },
                    &checked,
                )?;
            }
            let kind = if record.state == TransferState::ReadyToCommit {
                EventKind::ReceiverStaged
            } else {
                EventKind::ReceiverStageChecked
            };
            (record, kind)
        }
        Action::CommitTransfer {
            transfer_id,
            accept_degraded,
            lease_ms,
            ..
        } => {
            let mut record = record(c, task.task_id, transfer_id)?;
            if r.actor_id != record.binding.receiver_id {
                return Err(error(ErrorCode::Forbidden));
            }
            if record.state != TransferState::ReadyToCommit
                || !matches!(task.state, TaskState::Reserved | TaskState::Quiescing)
            {
                return Err(error(ErrorCode::Blocked));
            }
            if task.owner_id != Some(record.binding.source_id)
                || task.epoch != record.binding.source_epoch
            {
                return Err(error(ErrorCode::StaleEpoch));
            }
            let checked = facts(c, root, task, &record)?;
            let checkpoint = checkpoint(c, root, &record)?;
            let source = snapshots::repository(c, root, task)?;
            let vault = snapshots::vault(root)?;
            if let Some(grant) = grant(c, task)? {
                let stage = grant
                    .receiver_stage
                    .as_ref()
                    .ok_or_else(|| error(ErrorCode::CorruptStore))?;
                vault.verify_restored_source(
                    stage
                        .report
                        .as_ref()
                        .ok_or_else(|| error(ErrorCode::CorruptStore))?,
                    &stage.fingerprint,
                    &checkpoint.manifest_hash,
                )?;
            } else {
                vault.verify_source(&source, &checkpoint.manifest_hash)?;
            }
            let end = snapshots::finish(c, r, task, now, clock)?;
            record.advance(
                r.actor_id,
                r.expected_epoch,
                TransferStep::Commit { accept_degraded },
                &checked,
            )?;
            if record.state == TransferState::Committed {
                task.epoch = record
                    .committed_epoch
                    .ok_or_else(|| error(ErrorCode::CorruptStore))?;
                task.owner_id = Some(r.actor_id);
                task.lease_id = Some(Id::new_v4());
                task.expires_at_ms = Some(
                    end.checked_add(lease_ms)
                        .ok_or_else(|| error(ErrorCode::Blocked))?,
                );
                task.state = TaskState::Committed;
                save_task(c, task, r.expected_epoch)?;
            }
            let kind = if record.state == TransferState::Committed {
                EventKind::TransferCommitted
            } else {
                EventKind::TransferCommitChecked
            };
            (record, kind)
        }
        Action::RecoverTransfer { transfer_id, .. } => {
            let mut record = record(c, task.task_id, transfer_id)?;
            record.advance(
                r.actor_id,
                r.expected_epoch,
                TransferStep::Recover,
                &TransferFacts::default(),
            )?;
            if record.committed_epoch.is_some() {
                task.state = TaskState::RecoveryRequired;
                save_task(c, task, task.epoch)?;
            }
            (record, EventKind::TransferRecoveryRequired)
        }
        Action::ActivateReceiver { transfer_id, .. } => {
            let record = record(c, task.task_id, transfer_id)?;
            if r.actor_id != record.binding.receiver_id {
                return Err(error(ErrorCode::Forbidden));
            }
            return Err(error(ErrorCode::ExecutionNotEnabled));
        }
        Action::AbortTransfer { transfer_id, .. } => {
            let mut record = record(c, task.task_id, transfer_id)?;
            record.advance(
                r.actor_id,
                task.epoch,
                TransferStep::Abort,
                &TransferFacts::default(),
            )?;
            if task.state == TaskState::Quiescing {
                task.state = if grant(c, task)?.is_some() {
                    TaskState::RecoveryRequired
                } else {
                    TaskState::Suspect
                };
            }
            (record, EventKind::TransferAborted)
        }
        _ => return Err(error(ErrorCode::InvalidInput)),
    };
    persist(c, r, task, now, record, kind)
}

pub(super) fn pending_source(c: &Connection, task: &Task) -> Result<Option<Id>, Diagnostic> {
    let id: Option<String> = c.query_row("SELECT id FROM transfers WHERE task_id=?1 AND active=1 AND json_extract(record,'$.committed_epoch') IS NULL AND json_extract(record,'$.binding.source_epoch')=?2",
        params![task.task_id.to_string(), i64::try_from(task.epoch).map_err(|_| error(ErrorCode::CorruptStore))?], |row| row.get(0)).optional().db()?;
    id.map(|id| Id::parse_str(&id).map_err(|_| error(ErrorCode::CorruptStore)))
        .transpose()
}

pub(super) fn source_lost(
    c: &Connection,
    r: &Request,
    task: &mut Task,
    now: i64,
) -> Result<(), Diagnostic> {
    let id: String = c.query_row("SELECT id FROM transfers WHERE task_id=?1 AND active=1 AND json_extract(record,'$.binding.source_epoch')=?2",
        params![task.task_id.to_string(), i64::try_from(task.epoch).map_err(|_| error(ErrorCode::CorruptStore))?], |row| row.get(0)).db()?;
    let mut record = record(
        c,
        task.task_id,
        Id::parse_str(&id).map_err(|_| error(ErrorCode::CorruptStore))?,
    )?;
    record.advance(
        r.actor_id,
        task.epoch,
        TransferStep::Recover,
        &TransferFacts::default(),
    )?;
    task.state = expired_state(c, task)?;
    persist(c, r, task, now, record, EventKind::TransferRecoveryRequired)?;
    Ok(())
}

pub(super) fn receiver_lost(
    c: &Connection,
    r: &Request,
    task: &mut Task,
    run: Id,
    now: i64,
) -> Result<(), Diagnostic> {
    let transfer: String = c.query_row("SELECT id FROM transfers WHERE task_id=?1 AND json_extract(record,'$.receiver_activation.binding.run_id')=?2", params![task.task_id.to_string(), run.to_string()], |row| row.get(0)).db()?;
    let transfer = Id::parse_str(&transfer).map_err(|_| error(ErrorCode::CorruptStore))?;
    let mut record = record(c, task.task_id, transfer)?;
    if record.state == TransferState::RecoveryRequired {
        return Ok(());
    }
    record.advance(
        r.actor_id,
        task.epoch,
        TransferStep::Recover,
        &TransferFacts::default(),
    )?;
    task.state = TaskState::RecoveryRequired;
    persist(c, r, task, now, record, EventKind::TransferRecoveryRequired)?;
    Ok(())
}

pub(super) fn persist(
    c: &Connection,
    r: &Request,
    task: &mut Task,
    now: i64,
    record: TransferRecord,
    kind: EventKind,
) -> Result<Reply, Diagnostic> {
    let json = encode(&record)?;
    c.execute(
        "UPDATE transfers SET record=?1,active=?2 WHERE id=?3",
        params![
            json,
            holds_slot(&record),
            record.binding.transfer_id.to_string(),
        ],
    )
    .db()?;
    append_event(
        c,
        r,
        task,
        now,
        kind,
        None,
        Some(blake3::hash(json.as_bytes()).to_hex().as_ref()),
    )?;
    c.execute(
        "INSERT INTO transfer_events VALUES (?1,?2,?3)",
        params![
            record.binding.transfer_id.to_string(),
            i64::try_from(task.last_event_sequence).map_err(|_| error(ErrorCode::CorruptStore))?,
            json,
        ],
    )
    .db()?;
    Ok(Reply::Transfer(record))
}
