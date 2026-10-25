use super::*;
use baton_checkpoint::Vault;

pub(super) fn vault(root: &Path) -> Result<Vault, Diagnostic> {
    Vault::open(&root.join("checkpoints"))
}
pub(super) fn repository(
    c: &Connection,
    state_root: &Path,
    task: &Task,
) -> Result<PathBuf, Diagnostic> {
    let (mut root, common): (String, String) = c
        .query_row(
            "SELECT root, git_dir FROM repositories WHERE id=?1",
            [task.repository_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .db()?;
    if let Some(grant) = transfers::grant(c, task)? {
        let stage = grant
            .receiver_stage
            .as_ref()
            .ok_or_else(|| error(ErrorCode::CorruptStore))?;
        let report = stage
            .report
            .as_ref()
            .ok_or_else(|| error(ErrorCode::CorruptStore))?;
        if Some(report.restore_id) != grant.restore_id {
            return Err(error(ErrorCode::CorruptStore));
        }
        vault(state_root)?.verify_receiver_identity(report, &stage.fingerprint)?;
        root = report.worktree.clone();
    }
    let (actual, actual_common) = identify(Path::new(&root))?;
    if actual != Path::new(&root) || actual_common != Path::new(&common) {
        return Err(error(ErrorCode::SourceChanged));
    }
    Ok(actual)
}
pub(super) fn record(
    c: &Connection,
    root: &Path,
    task: Id,
    checkpoint: Id,
) -> Result<CheckpointRecord, Diagnostic> {
    let json: String = c
        .query_row(
            "SELECT record FROM checkpoints WHERE id=?1 AND task_id=?2",
            params![checkpoint.to_string(), task.to_string()],
            |r| r.get(0),
        )
        .optional()
        .db()?
        .ok_or_else(|| error(ErrorCode::NotFound))?;
    let record: CheckpointRecord = decode(&json)?;
    let manifest = vault(root)?.manifest(&record.manifest_hash)?;
    if record.binding != manifest.binding
        || record.binding.task_id != task
        || record.binding.checkpoint_id != checkpoint
        || record.binding.repository_id != load_task(c, task)?.repository_id
        || record.quality != RecoveryQuality::Blocked
        || record.reasons != manifest.reasons
    {
        return Err(error(ErrorCode::IntegrityFailure));
    }
    Ok(record)
}
pub(super) fn verify_reply(
    c: &Connection,
    root: &Path,
    request: &Request,
    reply: &Reply,
) -> Result<(), Diagnostic> {
    match reply {
        Reply::Run(cached)
            if matches!(
                &request.action,
                Action::ObserveRun {
                    observation: RunObservation::AdapterReported { .. }
                        | RunObservation::AdapterGap { .. },
                    ..
                }
            ) =>
        {
            execution::load(c, cached.binding.task_id, cached.binding.run_id)?;
        }
        Reply::Checkpoint(cached) => {
            if record(
                c,
                root,
                cached.binding.task_id,
                cached.binding.checkpoint_id,
            )? != *cached
            {
                return Err(error(ErrorCode::IntegrityFailure));
            }
        }
        Reply::Transfer(transfer)
            if matches!(
                request.action,
                Action::PreflightTransfer { .. }
                    | Action::BeginReceiverStaging { .. }
                    | Action::StageReceiver { .. }
                    | Action::CommitTransfer { .. }
            ) =>
        {
            if let Some(drain) = &transfer.source_drain {
                if native::drain_record(c, transfer.binding.task_id, drain.scope.binding.run_id)?
                    != *drain
                {
                    return Err(error(ErrorCode::CorruptStore));
                }
            }
            let checkpoint = transfer
                .checkpoint
                .as_ref()
                .ok_or_else(|| error(ErrorCode::CorruptStore))?;
            let actual = record(c, root, transfer.binding.task_id, checkpoint.checkpoint_id)?;
            if actual.manifest_hash != checkpoint.manifest_hash
                || actual.binding.event_cursor != checkpoint.event_cursor
                || actual.binding.epoch != transfer.binding.source_epoch
            {
                return Err(error(ErrorCode::IntegrityFailure));
            }
            let current = super::transfers::record(
                c,
                transfer.binding.task_id,
                transfer.binding.transfer_id,
            )?;
            // A cached stage/commit is historical; an activated receiver may have made new edits.
            if let Some(report) = transfer
                .receiver_stage
                .as_ref()
                .and_then(|stage| stage.report.as_ref())
                .filter(|_| current.receiver_activation.is_none())
            {
                let task = load_task(c, transfer.binding.task_id)?;
                vault(root)?.verify_restore(
                    &repository(c, root, &task)?,
                    &checkpoint.manifest_hash,
                    report,
                )?;
            }
        }
        Reply::Restored(report) => {
            let task = load_task(
                c,
                request
                    .action
                    .task_id()
                    .ok_or_else(|| error(ErrorCode::InvalidInput))?,
            )?;
            let checkpoint = record(c, root, task.task_id, report.checkpoint_id)?;
            vault(root)?.verify_restore(
                &repository(c, root, &task)?,
                &checkpoint.manifest_hash,
                report,
            )?;
        }
        _ => (),
    }
    Ok(())
}
pub(super) fn finish(
    c: &Connection,
    r: &Request,
    task: &mut Task,
    now: i64,
    clock: &dyn Clock,
) -> Result<i64, Diagnostic> {
    let sampled = clock.now_ms()?;
    if sampled < 0 {
        return Err(error(ErrorCode::Blocked));
    }
    let end = sampled.max(now);
    c.execute(
        "UPDATE clock SET last_ms=max(last_ms,?1) WHERE singleton=1",
        [end],
    )
    .db()?;
    if task.expires_at_ms.is_none_or(|expiry| end >= expiry) {
        task.state = expired_state(c, task)?;
        save_task(c, task, task.epoch)?;
        append_event(c, r, task, end, EventKind::LeaseSuspect, None, None)?;
        return Err(error(ErrorCode::Blocked));
    }
    Ok(end)
}
pub(super) fn apply(
    c: &Connection,
    root: &Path,
    r: &Request,
    task: &mut Task,
    now: i64,
    clock: &dyn Clock,
    local: bool,
) -> Result<Reply, Diagnostic> {
    let vault = vault(root)?;
    match &r.action {
        Action::CaptureCheckpoint {
            checkpoint_id,
            include_untracked,
            ..
        } => {
            if checkpoint_id.is_nil()
                || c.prepare("SELECT 1 FROM checkpoints WHERE id=?1")
                    .db()?
                    .exists([checkpoint_id.to_string()])
                    .db()?
            {
                return Err(error(ErrorCode::InvalidInput));
            }
            let parent: Option<String> = c
                .query_row(
                    "SELECT id FROM checkpoints WHERE task_id=?1 ORDER BY sequence DESC LIMIT 1",
                    [task.task_id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .db()?;
            let parent = parent
                .map(|p| Id::parse_str(&p).map_err(|_| error(ErrorCode::CorruptStore)))
                .transpose()?;
            let binding = SnapshotBinding {
                checkpoint_id: *checkpoint_id,
                parent_id: parent,
                repository_id: task.repository_id,
                task_id: task.task_id,
                actor_id: r.actor_id,
                operation_id: r.operation_id,
                epoch: task.epoch,
                event_cursor: task.last_event_sequence,
            };
            let source = repository(c, root, task)?;
            let hash = if let Some(grant) = transfers::grant(c, task)? {
                let stage = grant
                    .receiver_stage
                    .as_ref()
                    .ok_or_else(|| error(ErrorCode::CorruptStore))?;
                let report = stage
                    .report
                    .as_ref()
                    .ok_or_else(|| error(ErrorCode::CorruptStore))?;
                vault.capture_restored(
                    report,
                    &stage.fingerprint,
                    binding.clone(),
                    include_untracked,
                )?
            } else {
                vault.capture(&source, binding.clone(), include_untracked)?
            };
            let manifest = vault.manifest(&hash)?;
            let end = finish(c, r, task, now, clock)?;
            let record = CheckpointRecord {
                schema_version: SchemaVersion::V0_1_0,
                binding,
                manifest_hash: hash.clone(),
                quality: RecoveryQuality::Blocked,
                reasons: manifest.reasons,
            };
            append_event(
                c,
                r,
                task,
                end,
                EventKind::CheckpointPublished,
                None,
                Some(&hash),
            )?;
            c.execute(
                "INSERT INTO checkpoints VALUES (?1,?2,?3,?4,?5)",
                params![
                    checkpoint_id.to_string(),
                    task.task_id.to_string(),
                    i64::try_from(task.last_event_sequence)
                        .map_err(|_| error(ErrorCode::CorruptStore))?,
                    hash,
                    encode(&record)?
                ],
            )
            .db()?;
            Ok(Reply::Checkpoint(record))
        }
        Action::RestoreCheckpoint {
            checkpoint_id,
            restore_id,
            ..
        } => {
            if transfers::owns_restore(c, *restore_id)? {
                return Err(error(ErrorCode::IdempotencyConflict));
            }
            let record = record(c, root, task.task_id, *checkpoint_id)?;
            let fingerprint = request_fingerprint(r, local)?;
            restores::validate_intent(c, r, *restore_id, &record.manifest_hash, &fingerprint)?;
            let report = vault.restore_recoverable(
                &repository(c, root, task)?,
                &record.manifest_hash,
                *restore_id,
                &fingerprint,
            )?;
            let end = finish(c, r, task, now, clock)?;
            append_event(
                c,
                r,
                task,
                end,
                EventKind::CheckpointRestored,
                None,
                Some(&record.manifest_hash),
            )?;
            Ok(Reply::Restored(report))
        }
        Action::CollectOrphans { .. } => {
            if task.created_by != r.actor_id {
                return Err(error(ErrorCode::Forbidden));
            }
            let mut statement = c.prepare("SELECT manifest_hash FROM checkpoints").db()?;
            let hashes = statement
                .query_map([], |r| r.get::<_, String>(0))
                .db()?
                .collect::<Result<Vec<_>, _>>()
                .db()?;
            let removed = vault.collect_orphans(&hashes)?;
            let end = finish(c, r, task, now, clock)?;
            append_event(c, r, task, end, EventKind::OrphansCollected, None, None)?;
            Ok(Reply::OrphansCollected(removed))
        }
        _ => Err(error(ErrorCode::InvalidInput)),
    }
}
