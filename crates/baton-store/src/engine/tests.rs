use super::*;
use baton_test_support::GitFixture;
use std::process::Stdio;

struct FixedClock;
impl Clock for FixedClock {
    fn now_ms(&self) -> Result<i64, Diagnostic> {
        Ok(1_000)
    }
}

// M1-008; FR-04/19, INV-07: WAL/FULL is checked and busy writes stay unacknowledged.
#[test]
fn m1_008_wal_full_and_busy_retry() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let mut store = Store::create(&root, Arc::new(FixedClock)).unwrap();
    let actor = store.create_local_actor().unwrap();
    let repo = GitFixture::new().unwrap();
    let mode: String = store
        .connection
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    let sync: i64 = store
        .connection
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    assert_eq!(sync, 2);
    assert!(rusqlite::version_number() >= 3_051_003);
    store.connection.busy_timeout(Duration::ZERO).unwrap();
    let mut competing = Connection::open(root.join(FILE)).unwrap();
    let lock = competing
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let req = request(
        &actor,
        0,
        Action::EnrollRepository {
            repository_id: Id::new_v4(),
            root: repo.path().to_str().unwrap().into(),
        },
    );
    assert_eq!(
        store.execute(&actor, &req).unwrap_err().code,
        ErrorCode::StoreBusy
    );
    drop(lock);
    let reply = store.execute(&actor, &req).unwrap();
    assert_eq!(store.execute(&actor, &req).unwrap(), reply);
}
fn request(actor: &Actor, epoch: u64, action: Action) -> Request {
    Request {
        schema_version: SchemaVersion::V0_1_0,
        operation_id: Id::new_v4(),
        actor_id: actor.id(),
        expected_epoch: epoch,
        action,
    }
}

// M1-004; FR-04/07/08, INV-01/07: real kills must not acknowledge partial state.
#[test]
fn m1_004_hard_kill_before_and_after_commit_is_recoverable() {
    for point in ["before_commit", "after_commit"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let mut store = Store::create(&root, Arc::new(FixedClock)).unwrap();
        let actor = store.create_local_actor().unwrap();
        let repo = GitFixture::new().unwrap();
        let repo_id = Id::new_v4();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::EnrollRepository {
                        repository_id: repo_id,
                        root: repo.path().to_str().unwrap().into(),
                    },
                ),
            )
            .unwrap();
        let task = Id::new_v4();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::CreateTask {
                        task_id: task,
                        repository_id: repo_id,
                        goal: "Synthetic crash fixture".into(),
                        scope: "repository".into(),
                        agent: AgentMetadata {
                            name: "fake".into(),
                            version: "1".into(),
                        },
                    },
                ),
            )
            .unwrap();
        let req = request(
            &actor,
            0,
            Action::ClaimTask {
                task_id: task,
                lease_ms: 100,
            },
        );
        // No other SQLite connection remains to clean up the killed child's WAL.
        drop(store);
        kill_request(&root, &actor, &req, point, &temp.path().join("ready"));
        let mut recovered = Store::open(&root, Arc::new(FixedClock)).unwrap();
        let before = recovered.task(&actor, task).unwrap();
        let count: i64 = recovered
            .connection
            .query_row(
                "SELECT count(*) FROM operations WHERE id=?1",
                [req.operation_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        if point == "before_commit" {
            assert_eq!(before.epoch, 0);
            assert_eq!(before.state, TaskState::Unclaimed);
            assert_eq!(count, 0);
            assert_eq!(recovered.events(&actor, task).unwrap().len(), 1);
        } else {
            assert_eq!(before.epoch, 1);
            assert_eq!(before.state, TaskState::Reserved);
            assert_eq!(count, 1);
            assert_eq!(recovered.events(&actor, task).unwrap().len(), 2);
        }
        let result = recovered.execute(&actor, &req).unwrap();
        if let Reply::Lease(lease) = &result {
            assert_eq!(lease.epoch, 1);
            if point == "after_commit" {
                assert_eq!(Some(lease.lease_id), before.lease_id);
            }
        } else {
            panic!("expected lease");
        }
        assert_eq!(recovered.execute(&actor, &req).unwrap(), result);
        assert_eq!(recovered.events(&actor, task).unwrap().len(), 2);
    }
}

// M4-TR04; T-008/T-009: journal, task event and retry result commit together.
#[test]
fn m4_transfer_hard_kill_and_corrupt_pending_record() {
    for point in ["before_commit", "after_commit"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let mut store = Store::create(&root, Arc::new(FixedClock)).unwrap();
        let actor = store.create_local_actor().unwrap();
        let repo = GitFixture::new().unwrap();
        let repository_id = Id::new_v4();
        let task_id = Id::new_v4();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::EnrollRepository {
                        repository_id,
                        root: repo.path().to_str().unwrap().into(),
                    },
                ),
            )
            .unwrap();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::CreateTask {
                        repository_id,
                        task_id,
                        goal: "synthetic transfer crash".into(),
                        scope: ".".into(),
                        agent: AgentMetadata {
                            name: "fake".into(),
                            version: "1".into(),
                        },
                    },
                ),
            )
            .unwrap();
        let Reply::Lease(lease) = store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::ClaimTask {
                        task_id,
                        lease_ms: 100,
                    },
                ),
            )
            .unwrap()
        else {
            panic!()
        };
        let transfer_id = Id::new_v4();
        let req = request(
            &actor,
            1,
            Action::RequestTransfer {
                task_id,
                lease_id: lease.lease_id,
                transfer_id,
                receiver_id: actor.id(),
            },
        );
        drop(store);
        kill_request(&root, &actor, &req, point, &temp.path().join("ready"));
        let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
        if point == "before_commit" {
            assert_eq!(
                store
                    .transfer(&actor, task_id, transfer_id)
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        } else {
            assert_eq!(
                store.transfer(&actor, task_id, transfer_id).unwrap().state,
                TransferState::Quiescing
            );
        }
        let result = store.execute(&actor, &req).unwrap();
        assert_eq!(store.execute(&actor, &req).unwrap(), result);
        assert_eq!(store.events(&actor, task_id).unwrap().len(), 3);
        assert_eq!(store.task(&actor, task_id).unwrap().epoch, 1);
        // A damaged pending index must not reopen source launch/cancel paths.
        store
            .connection
            .execute(
                "UPDATE transfers SET active=0 WHERE id=?1",
                [transfer_id.to_string()],
            )
            .unwrap();
        assert_eq!(
            store
                .transfer(&actor, task_id, transfer_id)
                .unwrap_err()
                .code,
            ErrorCode::CorruptStore
        );
        let cancel = request(
            &actor,
            1,
            Action::CancelReservation {
                task_id,
                lease_id: lease.lease_id,
            },
        );
        assert_eq!(
            store.execute(&actor, &cancel).unwrap_err().code,
            ErrorCode::CorruptStore
        );
    }
}

// M4-RS02; T-004/T-005/T-008: restore intent and verified receipt survive hard kill.
#[test]
fn m4_restore_intent_and_completion_commit_crashes_are_recoverable() {
    for point in [
        "restore_intent_before_commit",
        "restore_intent_after_commit",
        "before_commit",
        "after_commit",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let mut store = Store::create(&root, Arc::new(FixedClock)).unwrap();
        let actor = store.create_local_actor().unwrap();
        let repo = GitFixture::new().unwrap();
        fs::write(repo.path().join("file"), b"synthetic source\n").unwrap();
        repo.git(&["add", "file"]).unwrap();
        repo.git(&["commit", "-m", "fixture"]).unwrap();
        let repository_id = Id::new_v4();
        let task_id = Id::new_v4();
        let checkpoint_id = Id::new_v4();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::EnrollRepository {
                        repository_id,
                        root: repo.path().to_str().unwrap().into(),
                    },
                ),
            )
            .unwrap();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::CreateTask {
                        task_id,
                        repository_id,
                        goal: "synthetic restore crash".into(),
                        scope: ".".into(),
                        agent: AgentMetadata {
                            name: "fake".into(),
                            version: "1".into(),
                        },
                    },
                ),
            )
            .unwrap();
        let Reply::Lease(lease) = store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::ClaimTask {
                        task_id,
                        lease_ms: 100,
                    },
                ),
            )
            .unwrap()
        else {
            panic!()
        };
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    1,
                    Action::CaptureCheckpoint {
                        task_id,
                        lease_id: lease.lease_id,
                        checkpoint_id,
                        include_untracked: vec![],
                    },
                ),
            )
            .unwrap();
        let restore_id = Id::new_v4();
        let req = request(
            &actor,
            1,
            Action::RestoreCheckpoint {
                task_id,
                lease_id: lease.lease_id,
                checkpoint_id,
                restore_id,
            },
        );
        drop(store);
        kill_request(&root, &actor, &req, point, &temp.path().join("ready"));
        let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
        let count: i64 = store
            .connection
            .query_row("SELECT count(*) FROM restore_intents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, i64::from(point != "restore_intent_before_commit"));
        let target = root
            .join("checkpoints/restores")
            .join(restore_id.to_string());
        assert_eq!(
            target.exists(),
            matches!(point, "before_commit" | "after_commit")
        );
        if point == "restore_intent_after_commit" {
            let mut stolen = request(
                &actor,
                1,
                Action::RecordIntent {
                    task_id,
                    lease_id: lease.lease_id,
                    summary: "conflicting operation".into(),
                },
            );
            stolen.operation_id = req.operation_id;
            assert_eq!(
                store.execute(&actor, &stolen).unwrap_err().code,
                ErrorCode::IdempotencyConflict
            );
        }
        let result = store.execute(&actor, &req).unwrap();
        assert_eq!(store.execute(&actor, &req).unwrap(), result);
        let events = store.events(&actor, task_id).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == EventKind::RestoreReserved)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == EventKind::CheckpointRestored)
                .count(),
            1
        );
        assert_eq!(
            fs::read(target.join("file")).unwrap(),
            b"synthetic source\n"
        );
        assert_eq!(
            fs::read(repo.path().join("file")).unwrap(),
            b"synthetic source\n"
        );
        assert_eq!(store.task(&actor, task_id).unwrap().epoch, 1);
        let mut competing = req.clone();
        competing.operation_id = Id::new_v4();
        assert_eq!(
            store.execute(&actor, &competing).unwrap_err().code,
            ErrorCode::IdempotencyConflict
        );
        assert_eq!(
            store.execute_local(&actor, &req).unwrap_err().code,
            ErrorCode::IdempotencyConflict
        );
        let count: i64 = store
            .connection
            .query_row("SELECT count(*) FROM restore_intents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}

fn kill_request(root: &Path, actor: &Actor, request: &Request, point: &str, ready: &Path) {
    kill_request_with_scope(root, actor, request, point, ready, None);
}
pub(super) fn kill_request_with_scope(
    root: &Path,
    actor: &Actor,
    request: &Request,
    point: &str,
    ready: &Path,
    scope: Option<Id>,
) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    if let Some(scope) = scope {
        command.env("BATON_STORE_TEST_BOUNDARY_TASK", scope.to_string());
    }
    let mut child = command
        .args([
            "--exact",
            "engine::tests::crash_child",
            "--ignored",
            "--nocapture",
        ])
        .env("BATON_STORE_TEST_ROOT", root)
        .env("BATON_STORE_TEST_CRASH", point)
        .env("BATON_STORE_TEST_READY", ready)
        .env("BATON_STORE_TEST_REQUEST", encode(request).unwrap())
        .env(
            "BATON_STORE_TEST_SECRET",
            serde_json::to_string(&actor.secret).unwrap(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while !ready.exists() && start.elapsed() < Duration::from_secs(30) {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("store child exited early: {status}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let reached = ready.exists();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(reached, "boundary {point} not reached");
}

// T-004-M2; FR-04/05, INV-03/07: actual DB commit kills never publish partial roots.
#[test]
fn t004_m2_kills_before_and_after_checkpoint_commit() {
    for point in ["before_commit", "after_commit"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let mut store = Store::create(&root, Arc::new(FixedClock)).unwrap();
        let actor = store.create_local_actor().unwrap();
        let repo = GitFixture::new().unwrap();
        fs::write(repo.path().join("file"), b"checkpoint data\n").unwrap();
        repo.git(&["add", "file"]).unwrap();
        repo.git(&["commit", "-m", "fixture"]).unwrap();
        let repository_id = Id::new_v4();
        let task_id = Id::new_v4();
        let checkpoint_id = Id::new_v4();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::EnrollRepository {
                        repository_id,
                        root: repo.path().to_str().unwrap().into(),
                    },
                ),
            )
            .unwrap();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::CreateTask {
                        task_id,
                        repository_id,
                        goal: "Checkpoint fixture".into(),
                        scope: "repository".into(),
                        agent: AgentMetadata {
                            name: "fake".into(),
                            version: "1".into(),
                        },
                    },
                ),
            )
            .unwrap();
        let lease = match store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::ClaimTask {
                        task_id,
                        lease_ms: 100,
                    },
                ),
            )
            .unwrap()
        {
            Reply::Lease(l) => l,
            _ => panic!(),
        };
        let req = request(
            &actor,
            1,
            Action::CaptureCheckpoint {
                task_id,
                lease_id: lease.lease_id,
                checkpoint_id,
                include_untracked: Vec::new(),
            },
        );
        drop(store);
        kill_request(&root, &actor, &req, point, &temp.path().join("ready"));
        let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
        if point == "before_commit" {
            assert_eq!(
                store
                    .checkpoint(&actor, task_id, checkpoint_id)
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            assert_eq!(store.task(&actor, task_id).unwrap().last_event_sequence, 2);
        } else {
            assert_eq!(
                store
                    .checkpoint(&actor, task_id, checkpoint_id)
                    .unwrap()
                    .binding
                    .epoch,
                1
            );
            assert_eq!(store.task(&actor, task_id).unwrap().last_event_sequence, 3);
        }
        let reply = store.execute(&actor, &req).unwrap();
        assert_eq!(store.execute(&actor, &req).unwrap(), reply);
        assert_eq!(store.task(&actor, task_id).unwrap().last_event_sequence, 3);
    }
}

// M1-004 subprocess entry, exercised only with synthetic parent-issued inputs.
#[test]
#[ignore = "subprocess entry point, invoked by M1/M2/M3 crash tests"]
fn crash_child() {
    let Some(root) = std::env::var_os("BATON_STORE_TEST_ROOT") else {
        return;
    };
    let req: Request =
        serde_json::from_str(&std::env::var("BATON_STORE_TEST_REQUEST").unwrap()).unwrap();
    let secret = serde_json::from_str(&std::env::var("BATON_STORE_TEST_SECRET").unwrap()).unwrap();
    let actor = Actor {
        id: req.actor_id,
        secret,
    };
    let mut store = Store::open(Path::new(&root), Arc::new(FixedClock)).unwrap();
    if let Ok(scope) = std::env::var("BATON_STORE_TEST_BOUNDARY_TASK") {
        transfers::simulate_boundary(Id::parse_str(&scope).unwrap());
    }
    if matches!(req.action, Action::AcknowledgeReceiver { .. }) {
        if std::env::var("BATON_STORE_TEST_CRASH")
            .is_ok_and(|point| point.starts_with("receiver_tool_"))
        {
            super::transfer_tests::crash_receiver_tool(&mut store, &actor, &req);
        } else {
            super::transfer_tests::crash_receiver_ready(&mut store, &actor, &req);
        }
    } else if let Action::ActivateReceiver {
        task_id,
        transfer_id,
    } = req.action
    {
        let transfer = store.transfer(&actor, task_id, transfer_id).unwrap();
        let report = transfer.receiver_stage.unwrap().report.unwrap();
        let spec = baton_platform::process::LaunchSpec::new(
            &std::env::current_exe().unwrap(),
            Path::new(&report.worktree),
            ["--help".into()],
        )
        .unwrap();
        let (_, permit) = store
            .prepare_receiver_launch(
                &actor,
                &req,
                req.operation_id,
                &spec,
                super::transfer_tests::receiver_capabilities(),
            )
            .unwrap();
        let permit = permit.unwrap();
        store.validate_launch(&actor, &permit).unwrap();
        let tree = baton_platform::process::ProcessTree::spawn(spec).unwrap();
        store
            .record_native_spawn(&actor, &permit, &tree, Id::new_v4())
            .unwrap();
    } else {
        store.execute(&actor, &req).unwrap();
    }
    panic!("fault boundary was not reached");
}

// M3-S03; FR-04/12, INV-07: crash after durable launch intent never respawns.
#[test]
fn m3_launch_commit_crashes_preserve_one_use_permission() {
    for point in ["before_commit", "after_commit"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let mut store = Store::create(&root, Arc::new(FixedClock)).unwrap();
        let actor = store.create_local_actor().unwrap();
        let repo = GitFixture::new().unwrap();
        let repository_id = Id::new_v4();
        let task_id = Id::new_v4();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::EnrollRepository {
                        repository_id,
                        root: repo.path().to_str().unwrap().into(),
                    },
                ),
            )
            .unwrap();
        store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::CreateTask {
                        task_id,
                        repository_id,
                        goal: "Launch crash fixture".into(),
                        scope: "temp".into(),
                        agent: AgentMetadata {
                            name: "fake".into(),
                            version: "1".into(),
                        },
                    },
                ),
            )
            .unwrap();
        let Reply::Lease(lease) = store
            .execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::ClaimTask {
                        task_id,
                        lease_ms: 100,
                    },
                ),
            )
            .unwrap()
        else {
            panic!()
        };
        let req = request(
            &actor,
            1,
            Action::OpenRun {
                task_id,
                lease_id: lease.lease_id,
                run_id: Id::new_v4(),
                launch_fingerprint: "c".repeat(64),
                capabilities: AdapterCapabilities {
                    lifecycle: true,
                    tool_outcomes: true,
                    interrupt: true,
                    mutation_gating: false,
                },
            },
        );
        drop(store);
        kill_request(&root, &actor, &req, point, &temp.path().join("ready"));
        let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
        let (_, permit) = store.prepare_launch(&actor, &req).unwrap();
        assert_eq!(permit.is_some(), point == "before_commit");
        assert!(store.prepare_launch(&actor, &req).unwrap().1.is_none());
        assert_eq!(store.task(&actor, task_id).unwrap().last_event_sequence, 3);
    }
}
