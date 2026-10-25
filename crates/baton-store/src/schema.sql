CREATE TABLE actors (id TEXT PRIMARY KEY, token_hash TEXT NOT NULL UNIQUE) STRICT;
CREATE TABLE local_principals (subject TEXT PRIMARY KEY, actor_id TEXT NOT NULL UNIQUE REFERENCES actors(id)) STRICT;
CREATE TABLE local_runtime (singleton INTEGER PRIMARY KEY CHECK(singleton=1), subject TEXT NOT NULL, session_id TEXT NOT NULL) STRICT;
CREATE TABLE repositories (
    id TEXT PRIMARY KEY, root TEXT NOT NULL, git_dir TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES actors(id), UNIQUE(root, git_dir)
) STRICT;
CREATE TABLE tasks (
    id TEXT PRIMARY KEY, repository_id TEXT NOT NULL REFERENCES repositories(id),
    record TEXT NOT NULL CHECK(json_valid(record))
) STRICT;
CREATE TABLE task_access (
    task_id TEXT NOT NULL REFERENCES tasks(id), actor_id TEXT NOT NULL REFERENCES actors(id),
    PRIMARY KEY(task_id, actor_id)
) STRICT;
CREATE TABLE events (
    task_id TEXT NOT NULL REFERENCES tasks(id), sequence INTEGER NOT NULL CHECK(sequence > 0),
    event_id TEXT NOT NULL UNIQUE, record TEXT NOT NULL CHECK(json_valid(record)),
    PRIMARY KEY(task_id, sequence)
) STRICT;
CREATE TABLE operations (
    id TEXT PRIMARY KEY, actor_id TEXT NOT NULL REFERENCES actors(id),
    fingerprint TEXT NOT NULL, result TEXT NOT NULL CHECK(json_valid(result))
) STRICT;
CREATE TABLE audit (
    sequence INTEGER PRIMARY KEY, record TEXT NOT NULL CHECK(json_valid(record))
) STRICT;
CREATE TABLE clock (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), last_ms INTEGER NOT NULL);
CREATE TABLE checkpoints (
    id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id),
    sequence INTEGER NOT NULL CHECK(sequence > 0), manifest_hash TEXT NOT NULL,
    record TEXT NOT NULL CHECK(json_valid(record)), UNIQUE(task_id, sequence)
) STRICT;
INSERT INTO clock VALUES (1, 0);
CREATE TABLE runs (
    id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id),
    record TEXT NOT NULL CHECK(json_valid(record)), event_count INTEGER NOT NULL CHECK(event_count >= 0)
) STRICT;
CREATE TABLE execution_events (
    run_id TEXT NOT NULL REFERENCES runs(id), sequence INTEGER NOT NULL CHECK(sequence > 0),
    record TEXT NOT NULL CHECK(json_valid(record)), PRIMARY KEY(run_id, sequence)
) STRICT;
CREATE TRIGGER execution_no_update BEFORE UPDATE ON execution_events BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER execution_no_delete BEFORE DELETE ON execution_events BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER events_no_update BEFORE UPDATE ON events BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER events_no_delete BEFORE DELETE ON events BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER operations_no_update BEFORE UPDATE ON operations BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER operations_no_delete BEFORE DELETE ON operations BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER audit_no_delete BEFORE DELETE ON audit BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER checkpoints_no_update BEFORE UPDATE ON checkpoints BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER checkpoints_no_delete BEFORE DELETE ON checkpoints BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TABLE transfers (
    id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id),
    active INTEGER NOT NULL CHECK(active IN (0,1)),
    record TEXT NOT NULL CHECK(json_valid(record))
) STRICT;
CREATE UNIQUE INDEX one_pending_transfer ON transfers(task_id) WHERE active=1;
CREATE TABLE transfer_events (
    transfer_id TEXT NOT NULL REFERENCES transfers(id), sequence INTEGER NOT NULL,
    record TEXT NOT NULL CHECK(json_valid(record)), PRIMARY KEY(transfer_id, sequence)
) STRICT;
CREATE TRIGGER transfer_events_no_update BEFORE UPDATE ON transfer_events BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER transfer_events_no_delete BEFORE DELETE ON transfer_events BEGIN SELECT RAISE(ABORT, 'append only'); END;
PRAGMA application_id = 1111577678;
CREATE TABLE restore_intents (
    operation_id TEXT PRIMARY KEY, restore_id TEXT NOT NULL UNIQUE,
    task_id TEXT NOT NULL REFERENCES tasks(id), actor_id TEXT NOT NULL REFERENCES actors(id),
    fingerprint TEXT NOT NULL, manifest_hash TEXT NOT NULL
) STRICT;
CREATE TRIGGER restore_intents_no_update BEFORE UPDATE ON restore_intents BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER restore_intents_no_delete BEFORE DELETE ON restore_intents BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TABLE native_scopes (
    run_id TEXT PRIMARY KEY REFERENCES runs(id), scope_id TEXT NOT NULL UNIQUE,
    task_id TEXT NOT NULL REFERENCES tasks(id), sequence INTEGER NOT NULL,
    record TEXT NOT NULL CHECK(json_valid(record))
) STRICT;
CREATE TABLE native_drains (
    run_id TEXT PRIMARY KEY REFERENCES native_scopes(run_id), task_id TEXT NOT NULL REFERENCES tasks(id),
    sequence INTEGER NOT NULL, record TEXT NOT NULL CHECK(json_valid(record))
) STRICT;
CREATE TRIGGER native_scopes_no_update BEFORE UPDATE ON native_scopes BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER native_scopes_no_delete BEFORE DELETE ON native_scopes BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER native_drains_no_update BEFORE UPDATE ON native_drains BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE TRIGGER native_drains_no_delete BEFORE DELETE ON native_drains BEGIN SELECT RAISE(ABORT, 'immutable'); END;
CREATE UNIQUE INDEX receiver_restore_identity ON transfers(json_extract(record,'$.restore_id')) WHERE json_extract(record,'$.restore_id') IS NOT NULL;
CREATE UNIQUE INDEX receiver_stage_identity ON transfers(json_extract(record,'$.receiver_stage.operation_id')) WHERE json_extract(record,'$.receiver_stage.operation_id') IS NOT NULL;
CREATE UNIQUE INDEX one_run_per_epoch ON runs(task_id,json_extract(record,'$.binding.epoch'));
CREATE UNIQUE INDEX receiver_session_identity ON transfers(json_extract(record,'$.receiver_ready.session.session_id')) WHERE json_extract(record,'$.receiver_ready.session.session_id') IS NOT NULL;
CREATE UNIQUE INDEX one_committed_grant_per_epoch ON transfers(task_id,json_extract(record,'$.committed_epoch')) WHERE json_extract(record,'$.committed_epoch') IS NOT NULL;
PRAGMA user_version = 15;
