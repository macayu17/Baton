use super::*;

impl Store {
    // Persist intent separately: filesystem work cannot roll back with a SQLite transaction.
    pub(super) fn reserve_restore(
        &mut self,
        actor: &Actor,
        r: &Request,
        local: bool,
    ) -> Result<(), Diagnostic> {
        let Action::RestoreCheckpoint {
            task_id,
            lease_id,
            checkpoint_id,
            restore_id,
        } = r.action
        else {
            return Ok(());
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .db()?;
        authenticate(&tx, actor)?;
        if actor.id != r.actor_id {
            return Err(error(ErrorCode::Unauthorized));
        }
        if tx
            .prepare("SELECT 1 FROM operations WHERE id=?1")
            .db()?
            .exists([r.operation_id.to_string()])
            .db()?
        {
            return Ok(());
        }
        if transfers::owns_restore(&tx, restore_id)? || tx.prepare(
            "SELECT 1 FROM transfers WHERE json_extract(record,'$.receiver_stage.operation_id')=?1")
            .db()?.exists([r.operation_id.to_string()]).db()? { return Ok(()); }
        // The normal request transaction records semantic denials and idempotent errors.
        if validate(r).is_err()
            || (local && !local_mutation_allowed(&r.action))
            || access(&tx, r.actor_id, task_id).is_err()
        {
            return Ok(());
        }
        let mut task = load_task(&tx, task_id)?;
        let sampled = self.clock.now_ms()?;
        if sampled < 0 {
            return Err(error(ErrorCode::Blocked));
        }
        let high: i64 = tx
            .query_row("SELECT last_ms FROM clock WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .db()?;
        let now = sampled.max(high);
        if task.epoch != r.expected_epoch
            || task.state != TaskState::Reserved
            || task.owner_id != Some(r.actor_id)
            || task.lease_id != Some(lease_id)
            || task.expires_at_ms.is_none_or(|expiry| now >= expiry)
        {
            return Ok(());
        }
        if tx
            .prepare("SELECT 1 FROM restore_intents WHERE operation_id=?1 OR restore_id=?2")
            .db()?
            .exists(params![r.operation_id.to_string(), restore_id.to_string()])
            .db()?
        {
            return Ok(());
        }
        let checkpoint = match snapshots::record(&tx, &self.root, task_id, checkpoint_id) {
            Ok(record) => record,
            Err(_) => return Ok(()),
        };
        let fingerprint = request_fingerprint(r, local)?;
        tx.execute(
            "INSERT INTO restore_intents VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                r.operation_id.to_string(),
                restore_id.to_string(),
                task_id.to_string(),
                actor.id.to_string(),
                fingerprint,
                checkpoint.manifest_hash,
            ],
        )
        .db()?;
        tx.execute("UPDATE clock SET last_ms=?1 WHERE singleton=1", [now])
            .db()?;
        append_event(
            &tx,
            r,
            &mut task,
            now,
            EventKind::RestoreReserved,
            None,
            Some(&fingerprint),
        )?;
        audit(&tx, r, now, None)?;
        #[cfg(test)]
        crash_point("restore_intent_before_commit");
        tx.commit().db()?;
        #[cfg(test)]
        crash_point("restore_intent_after_commit");
        Ok(())
    }
}

pub(super) fn validate_intent(
    c: &Connection,
    r: &Request,
    restore: Id,
    manifest: &str,
    fingerprint: &str,
) -> Result<(), Diagnostic> {
    let actual: Option<(String, String, String, String, String)> = c.query_row(
        "SELECT operation_id,task_id,actor_id,fingerprint,manifest_hash FROM restore_intents WHERE restore_id=?1",
        [restore.to_string()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
    ).optional().db()?;
    let actual = actual.ok_or_else(|| error(ErrorCode::Blocked))?;
    if actual
        != (
            r.operation_id.to_string(),
            r.action
                .task_id()
                .ok_or_else(|| error(ErrorCode::InvalidInput))?
                .to_string(),
            r.actor_id.to_string(),
            fingerprint.into(),
            manifest.into(),
        )
    {
        return Err(error(ErrorCode::IdempotencyConflict));
    }
    Ok(())
}
