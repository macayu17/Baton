use super::*;

impl Store {
    pub fn publish_local_session(
        &mut self,
        authority: &baton_platform::LocalAuthority,
        session_id: Id,
    ) -> Result<(), Diagnostic> {
        if authority.root() != self.root
            || session_id.is_nil()
            || authority.subject() != baton_platform::current_principal()?
        {
            return Err(error(ErrorCode::Forbidden));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .db()?;
        let timestamp_ms = bootstrap_time(&tx, self.clock.as_ref())?;
        let actor: String = tx
            .query_row(
                "SELECT actor_id FROM local_principals WHERE subject=?1",
                [authority.subject()],
                |r| r.get(0),
            )
            .db()?;
        tx.execute("INSERT INTO local_runtime VALUES (1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET subject=excluded.subject,session_id=excluded.session_id", params![authority.subject(), session_id.to_string()]).db()?;
        let entry = AuditEntry {
            schema_version: SchemaVersion::V0_1_0,
            audit_id: Id::new_v4(),
            actor_id: Id::parse_str(&actor).map_err(|_| error(ErrorCode::CorruptStore))?,
            operation_id: session_id,
            task_id: None,
            expected_epoch: 0,
            timestamp_ms,
            reason: None,
            kind: AuditKind::LocalDaemonBound,
        };
        tx.execute("INSERT INTO audit(record) VALUES (?1)", [encode(&entry)?])
            .db()?;
        tx.commit().db()
    }

    /// Read only the endpoint locator under OS checks; no actor token is exposed.
    pub fn published_local_session(root: &Path) -> Result<Id, Diagnostic> {
        baton_platform::validate_private(root)?;
        baton_platform::validate_private(&root.join(FILE))?;
        let c = Connection::open_with_flags(
            root.join(FILE),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .db()?;
        c.busy_timeout(Duration::from_secs(3)).db()?;
        let app: i64 = c
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .db()?;
        let schema: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .db()?;
        if app != APPLICATION_ID || schema != 15 {
            return Err(error(ErrorCode::UnsupportedSchema));
        }
        let (subject, session): (String, String) = c
            .query_row(
                "SELECT subject,session_id FROM local_runtime WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .db()?
            .ok_or_else(|| error(ErrorCode::NotFound))?;
        if subject != baton_platform::current_principal()? {
            return Err(error(ErrorCode::Forbidden));
        }
        Id::parse_str(&session).map_err(|_| error(ErrorCode::CorruptStore))
    }

    /// Trusted daemon bootstrap under an exclusive OS-derived local authority.
    /// Recover a stable actor ID and rotate its in-memory capability on restart.
    pub fn recover_local_identity(
        &mut self,
        authority: &baton_platform::LocalAuthority,
    ) -> Result<Actor, Diagnostic> {
        if authority.root() != self.root
            || authority.subject() != baton_platform::current_principal()?
        {
            return Err(error(ErrorCode::Forbidden));
        }
        baton_platform::validate_private(&self.root)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .db()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT actor_id FROM local_principals WHERE subject=?1",
                [authority.subject()],
                |r| r.get(0),
            )
            .optional()
            .db()?;
        let actor = Actor::fresh(
            existing
                .as_deref()
                .map(Id::parse_str)
                .transpose()
                .map_err(|_| error(ErrorCode::CorruptStore))?
                .unwrap_or_else(Id::new_v4),
        );
        if existing.is_some() {
            if tx
                .execute(
                    "UPDATE actors SET token_hash=?1 WHERE id=?2",
                    params![actor.hash(), actor.id.to_string()],
                )
                .db()?
                != 1
            {
                return Err(error(ErrorCode::CorruptStore));
            }
        } else {
            tx.execute(
                "INSERT INTO actors VALUES (?1,?2)",
                params![actor.id.to_string(), actor.hash()],
            )
            .db()?;
            tx.execute(
                "INSERT INTO local_principals VALUES (?1,?2)",
                params![authority.subject(), actor.id.to_string()],
            )
            .db()?;
        }
        let timestamp_ms = bootstrap_time(&tx, self.clock.as_ref())?;
        let entry = AuditEntry {
            schema_version: SchemaVersion::V0_1_0,
            audit_id: Id::new_v4(),
            actor_id: actor.id,
            operation_id: Id::new_v4(),
            task_id: None,
            expected_epoch: 0,
            timestamp_ms,
            reason: None,
            kind: AuditKind::LocalIdentityRecovered,
        };
        tx.execute("INSERT INTO audit(record) VALUES (?1)", [encode(&entry)?])
            .db()?;
        tx.commit().db()?;
        Ok(actor)
    }
}

fn bootstrap_time(c: &Connection, clock: &dyn Clock) -> Result<i64, Diagnostic> {
    let sampled = clock.now_ms()?;
    if sampled < 0 {
        return Err(error(ErrorCode::Blocked));
    }
    let last: i64 = c
        .query_row("SELECT last_ms FROM clock WHERE singleton=1", [], |r| {
            r.get(0)
        })
        .db()?;
    let now = sampled.max(last);
    c.execute("UPDATE clock SET last_ms=?1 WHERE singleton=1", [now])
        .db()?;
    Ok(now)
}
