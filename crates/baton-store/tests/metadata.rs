use baton_protocol::*;
use baton_store::{Actor, Clock, Store};
use baton_test_support::GitFixture;
use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc, Barrier,
};

struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn now_ms(&self) -> Result<i64, Diagnostic> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct Fixture {
    path: std::path::PathBuf,
    repo: GitFixture,
    clock: Arc<TestClock>,
    store: Store,
    actor: Actor,
    repo_id: Id,
    _temp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state");
        let clock = Arc::new(TestClock(AtomicI64::new(1_000)));
        let mut store = Store::create(&path, clock.clone()).unwrap();
        let actor = store.create_local_actor().unwrap();
        let repo = GitFixture::new().unwrap();
        let repo_id = Id::new_v4();
        let req = request(
            &actor,
            0,
            Action::EnrollRepository {
                repository_id: repo_id,
                root: repo.path().to_str().unwrap().into(),
            },
        );
        store.execute(&actor, &req).unwrap();
        Self {
            _temp: temp,
            path,
            repo,
            clock,
            store,
            actor,
            repo_id,
        }
    }
    fn task(&mut self) -> Id {
        let id = Id::new_v4();
        self.store
            .execute(
                &self.actor,
                &request(
                    &self.actor,
                    0,
                    Action::CreateTask {
                        task_id: id,
                        repository_id: self.repo_id,
                        goal: "Fix a synthetic failing test".into(),
                        scope: "repository".into(),
                        agent: AgentMetadata {
                            name: "fake-agent".into(),
                            version: "test".into(),
                        },
                    },
                ),
            )
            .unwrap();
        id
    }
    fn claim(&mut self, task: Id, epoch: u64) -> Lease {
        match self
            .store
            .execute(
                &self.actor,
                &request(
                    &self.actor,
                    epoch,
                    Action::ClaimTask {
                        task_id: task,
                        lease_ms: 100,
                    },
                ),
            )
            .unwrap()
        {
            Reply::Lease(lease) => lease,
            other => panic!("unexpected reply: {other:?}"),
        }
    }
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
// M4-NP03; FR-12, INV-02/09: synthetic status cannot mint native scope/drain evidence.
#[test]
fn m4_synthetic_tree_stopped_never_creates_native_drain() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let run = Id::new_v4();
    f.store
        .execute(
            &f.actor,
            &request(
                &f.actor,
                1,
                Action::OpenRun {
                    task_id: task,
                    lease_id: lease.lease_id,
                    run_id: run,
                    launch_fingerprint: "a".repeat(64),
                    capabilities: AdapterCapabilities {
                        lifecycle: true,
                        tool_outcomes: true,
                        interrupt: true,
                        mutation_gating: true,
                    },
                },
            ),
        )
        .unwrap();
    for observation in [
        RunObservation::Spawned { pid: 1 },
        RunObservation::TreeStopped,
    ] {
        f.store
            .execute(
                &f.actor,
                &request(
                    &f.actor,
                    1,
                    Action::ObserveRun {
                        task_id: task,
                        lease_id: lease.lease_id,
                        run_id: run,
                        observation,
                    },
                ),
            )
            .unwrap();
    }
    assert!(f.store.run(&f.actor, task, run).unwrap().tree_stopped);
    assert_eq!(
        f.store.native_drain(&f.actor, task, run).unwrap_err().code,
        ErrorCode::NotFound
    );
}

// M4-NP04; FR-12/13, INV-09: native seals cannot hide a missing execution journal.
#[cfg(windows)]
#[test]
fn m4_native_drain_detects_missing_execution_evidence() {
    use baton_platform::process::{LaunchSpec, ProcessTree, TreeStatus};
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let spec = LaunchSpec::new(
        &std::env::current_exe().unwrap(),
        f._temp.path(),
        ["--help".into()],
    )
    .unwrap();
    let run_id = Id::new_v4();
    let request = request(
        &f.actor,
        1,
        Action::OpenRun {
            task_id: task,
            lease_id: lease.lease_id,
            run_id,
            launch_fingerprint: spec.fingerprint(),
            capabilities: AdapterCapabilities {
                lifecycle: false,
                tool_outcomes: false,
                interrupt: false,
                mutation_gating: false,
            },
        },
    );
    let (_, permit) = f.store.prepare_launch(&f.actor, &request).unwrap();
    let permit = permit.unwrap();
    let mut tree = ProcessTree::spawn(spec).unwrap();
    let spawn_operation = Id::new_v4();
    let spawned = f
        .store
        .record_native_spawn(&f.actor, &permit, &tree, spawn_operation)
        .unwrap();
    assert_eq!(
        f.store
            .record_native_spawn(&f.actor, &permit, &tree, spawn_operation)
            .unwrap(),
        spawned
    );
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while tree.status().unwrap() != TreeStatus::Empty {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let proof = tree.drain_proof().unwrap();
    let drain_operation = Id::new_v4();
    let stopped = f
        .store
        .record_native_drain(&f.actor, &permit.record().binding, &proof, drain_operation)
        .unwrap();
    assert_eq!(
        f.store
            .record_native_drain(&f.actor, &permit.record().binding, &proof, drain_operation)
            .unwrap(),
        stopped
    );
    f.store.native_drain(&f.actor, task, run_id).unwrap();
    let c = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    c.execute_batch("DROP TRIGGER execution_no_delete").unwrap();
    c.execute(
        "DELETE FROM execution_events WHERE run_id=?1 AND sequence=2",
        [run_id.to_string()],
    )
    .unwrap();
    assert_eq!(
        f.store
            .native_drain(&f.actor, task, run_id)
            .unwrap_err()
            .code,
        ErrorCode::CorruptStore
    );
}

fn code(result: Result<Reply, Diagnostic>) -> ErrorCode {
    result.unwrap_err().code
}

// M4-TR01; T-003/T-008: persisted preflight cannot activate or change ownership.
#[test]
fn m4_transfer_request_retry_restart_preflight_and_abort() {
    let mut f = Fixture::new();
    seed_checkpoint_repo(&f);
    let task = f.task();
    let lease = f.claim(task, 0);
    let transfer = Id::new_v4();
    let req = request(
        &f.actor,
        1,
        Action::RequestTransfer {
            task_id: task,
            lease_id: lease.lease_id,
            transfer_id: transfer,
            receiver_id: f.actor.id(),
        },
    );
    let initial = f.store.execute_local(&f.actor, &req).unwrap();
    assert_eq!(initial, f.store.execute_local(&f.actor, &req).unwrap());
    f.store = Store::open(&f.path, f.clock.clone()).unwrap();
    assert_eq!(initial, f.store.execute_local(&f.actor, &req).unwrap());
    assert_eq!(
        f.store.transfer(&f.actor, task, transfer).unwrap().state,
        TransferState::Quiescing
    );
    let competing = request(
        &f.actor,
        1,
        Action::RequestTransfer {
            task_id: task,
            lease_id: lease.lease_id,
            transfer_id: Id::new_v4(),
            receiver_id: f.actor.id(),
        },
    );
    assert_eq!(
        code(f.store.execute(&f.actor, &competing)),
        ErrorCode::Blocked
    );
    let checkpoint_id = Id::new_v4();
    f.store
        .execute(
            &f.actor,
            &request(
                &f.actor,
                1,
                Action::CaptureCheckpoint {
                    task_id: task,
                    lease_id: lease.lease_id,
                    checkpoint_id,
                    include_untracked: vec![],
                },
            ),
        )
        .unwrap();
    let preflight = request(
        &f.actor,
        1,
        Action::PreflightTransfer {
            task_id: task,
            lease_id: lease.lease_id,
            transfer_id: transfer,
            checkpoint_id,
        },
    );
    let Reply::Transfer(blocked) = f.store.execute_local(&f.actor, &preflight).unwrap() else {
        panic!()
    };
    assert_eq!(blocked.state, TransferState::Blocked);
    assert!(blocked
        .reasons
        .contains(&TransferReason::SourceFenceUnavailable));
    assert!(blocked
        .reasons
        .contains(&TransferReason::AdapterUnvalidated));
    assert_eq!(blocked.committed_epoch, None);
    let manifest_path = f
        .path
        .join("checkpoints/manifests")
        .join(&blocked.checkpoint.as_ref().unwrap().manifest_hash);
    let original = std::fs::read(&manifest_path).unwrap();
    std::fs::write(&manifest_path, b"corrupt synthetic checkpoint").unwrap();
    assert_eq!(
        code(f.store.execute_local(&f.actor, &preflight)),
        ErrorCode::IntegrityFailure
    );
    std::fs::write(&manifest_path, original).unwrap();
    assert_eq!(f.store.task(&f.actor, task).unwrap().epoch, 1);
    assert_eq!(
        code(f.store.execute(
            &f.actor,
            &request(
                &f.actor,
                1,
                Action::CancelReservation {
                    task_id: task,
                    lease_id: lease.lease_id,
                }
            )
        )),
        ErrorCode::Blocked
    );
    let abort = request(
        &f.actor,
        1,
        Action::AbortTransfer {
            task_id: task,
            transfer_id: transfer,
        },
    );
    let Reply::Transfer(aborted) = f.store.execute_local(&f.actor, &abort).unwrap() else {
        panic!()
    };
    assert_eq!(aborted.state, TransferState::FailedPrecommit);
    assert_eq!(
        f.store.execute_local(&f.actor, &abort).unwrap(),
        Reply::Transfer(aborted)
    );
    assert_eq!(
        f.store.task(&f.actor, task).unwrap().owner_id,
        Some(f.actor.id())
    );
}

// M4-TR02: foreign actors, stale epochs and expired reservations never transfer.
#[test]
fn m4_transfer_auth_epoch_expiry_and_unknown_proof_fields() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let foreign = f.store.create_local_actor().unwrap();
    let action = Action::RequestTransfer {
        task_id: task,
        lease_id: lease.lease_id,
        transfer_id: Id::new_v4(),
        receiver_id: foreign.id(),
    };
    assert_eq!(
        code(
            f.store
                .execute(&foreign, &request(&foreign, 1, action.clone()))
        ),
        ErrorCode::Forbidden
    );
    assert_eq!(
        code(
            f.store
                .execute(&f.actor, &request(&f.actor, 0, action.clone()))
        ),
        ErrorCode::StaleEpoch
    );
    assert_eq!(
        code(
            f.store
                .execute(&f.actor, &request(&f.actor, 1, action.clone()))
        ),
        ErrorCode::Forbidden
    );
    let mut json = serde_json::to_value(&action).unwrap();
    json["source_fenced"] = true.into();
    assert!(serde_json::from_value::<Action>(json).is_err());
    f.clock.0.store(1_101, Ordering::SeqCst);
    assert_eq!(
        code(f.store.execute(&f.actor, &request(&f.actor, 1, action))),
        ErrorCode::Blocked
    );
    assert_eq!(f.store.task(&f.actor, task).unwrap().epoch, 1);
}

// M4-TR03: competing requests cannot create two pending receivers.
#[test]
fn m4_transfer_concurrent_requests_and_pending_launch_permit() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let launch = request(
        &f.actor,
        1,
        Action::OpenRun {
            task_id: task,
            lease_id: lease.lease_id,
            run_id: Id::new_v4(),
            launch_fingerprint: "a".repeat(64),
            capabilities: AdapterCapabilities {
                lifecycle: true,
                tool_outcomes: true,
                interrupt: true,
                mutation_gating: false,
            },
        },
    );
    let (_, permit) = f.store.prepare_launch(&f.actor, &launch).unwrap();
    let permit = permit.unwrap();
    f.store.validate_launch(&f.actor, &permit).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let mut store = Store::open(&f.path, f.clock.clone()).unwrap();
            let actor = f.actor.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.execute(
                    &actor,
                    &request(
                        &actor,
                        1,
                        Action::RequestTransfer {
                            task_id: task,
                            lease_id: lease.lease_id,
                            transfer_id: Id::new_v4(),
                            receiver_id: actor.id(),
                        },
                    ),
                )
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(e) if e.code == ErrorCode::Blocked))
            .count(),
        1
    );
    assert_eq!(
        f.store.validate_launch(&f.actor, &permit).unwrap_err().code,
        ErrorCode::Blocked
    );
}

// M4-TR05: expiry permits aborting intent, never ownership revival.
#[test]
fn m4_transfer_expired_source_can_abort_without_renewal() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let transfer_id = Id::new_v4();
    f.store
        .execute(
            &f.actor,
            &request(
                &f.actor,
                1,
                Action::RequestTransfer {
                    task_id: task,
                    lease_id: lease.lease_id,
                    transfer_id,
                    receiver_id: f.actor.id(),
                },
            ),
        )
        .unwrap();
    f.clock.0.store(1_101, Ordering::SeqCst);
    let abort = request(
        &f.actor,
        1,
        Action::AbortTransfer {
            task_id: task,
            transfer_id,
        },
    );
    f.store.execute(&f.actor, &abort).unwrap();
    let task = f.store.task(&f.actor, task).unwrap();
    assert_eq!(task.state, TaskState::Suspect);
    assert_eq!(task.epoch, 1);
    assert_eq!(task.expires_at_ms, Some(1_100));
}

#[test]
fn m4_previous_schemas_are_preserved_without_migration() {
    let f = Fixture::new();
    let c = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    for schema in [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14] {
        c.pragma_update(None, "user_version", schema).unwrap();
        assert_eq!(
            Store::open(&f.path, f.clock.clone()).err().unwrap().code,
            ErrorCode::UnsupportedSchema
        );
        assert_eq!(
            c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            schema
        );
    }
}

// M4-ID01; FR-12, INV-07: OS identity survives restart without a saved bearer token.
#[test]
fn m4_os_identity_recovery_preserves_actor_and_revokes_previous_capability() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let clock = Arc::new(TestClock(AtomicI64::new(1000)));
    let mut store = Store::create(&root, clock.clone()).unwrap();
    let authority = baton_platform::LocalAuthority::acquire(&root).unwrap();
    let actor = store.recover_local_identity(&authority).unwrap();
    let id = actor.id();
    drop(store);
    drop(authority);
    let mut store = Store::open(&root, clock).unwrap();
    let authority = baton_platform::LocalAuthority::acquire(&root).unwrap();
    let recovered = store.recover_local_identity(&authority).unwrap();
    assert_eq!(recovered.id(), id);
    let repo = GitFixture::new().unwrap();
    let req = request(
        &actor,
        0,
        Action::EnrollRepository {
            repository_id: Id::new_v4(),
            root: repo.path().to_str().unwrap().into(),
        },
    );
    assert_eq!(code(store.execute(&actor, &req)), ErrorCode::Unauthorized);
    store.execute(&recovered, &req).unwrap();
    assert!(!root.join("credentials").exists());
}

// M4-ID02; FR-12/13, INV-02/07: local callers cannot replay trusted operation grants.
#[test]
fn m4_local_and_trusted_operation_domains_are_distinct() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let req = request(
        &f.actor,
        1,
        Action::OpenRun {
            task_id: task,
            lease_id: lease.lease_id,
            run_id: Id::new_v4(),
            launch_fingerprint: "a".repeat(64),
            capabilities: AdapterCapabilities {
                lifecycle: false,
                tool_outcomes: false,
                interrupt: false,
                mutation_gating: false,
            },
        },
    );
    f.store.execute(&f.actor, &req).unwrap();
    assert_eq!(
        f.store.execute_local(&f.actor, &req).unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    let mut fresh = req;
    fresh.operation_id = Id::new_v4();
    assert_eq!(
        f.store.execute_local(&f.actor, &fresh).unwrap_err().code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.store.execute_local(&f.actor, &fresh).unwrap_err().code,
        ErrorCode::Forbidden
    );
}

// M3-S01; FR-03/12, INV-06/07: committed launch retries cannot mint permits.
#[test]
fn m3_launch_permit_is_fresh_only_and_pending_launch_blocks_cancellation() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let run = Id::new_v4();
    let req = request(
        &f.actor,
        1,
        Action::OpenRun {
            task_id: task,
            lease_id: lease.lease_id,
            run_id: run,
            launch_fingerprint: "a".repeat(64),
            capabilities: AdapterCapabilities {
                lifecycle: true,
                tool_outcomes: true,
                interrupt: true,
                mutation_gating: false,
            },
        },
    );
    assert!(f.store.prepare_launch(&f.actor, &req).unwrap().1.is_some());
    assert!(f.store.prepare_launch(&f.actor, &req).unwrap().1.is_none());
    let cancel = request(
        &f.actor,
        1,
        Action::CancelReservation {
            task_id: task,
            lease_id: lease.lease_id,
        },
    );
    assert_eq!(code(f.store.execute(&f.actor, &cancel)), ErrorCode::Blocked);
    let mut reopened = Store::open(&f.path, f.clock.clone()).unwrap();
    assert!(reopened.prepare_launch(&f.actor, &req).unwrap().1.is_none());
    assert_eq!(
        reopened.run(&f.actor, task, run).unwrap().state,
        RunState::LaunchPending
    );
    let loss = request(
        &f.actor,
        1,
        Action::ObserveRun {
            task_id: task,
            lease_id: lease.lease_id,
            run_id: run,
            observation: RunObservation::SupervisorLost,
        },
    );
    reopened.execute(&f.actor, &loss).unwrap();
    assert_eq!(
        reopened.run(&f.actor, task, run).unwrap().state,
        RunState::RecoveryRequired
    );
}

// T-007-M3; FR-14, INV-04/09: effect uncertainty survives restart and retry.
#[test]
fn m3_execution_journal_retains_uncertain_external_effects_and_gaps() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let run = Id::new_v4();
    f.store
        .execute(
            &f.actor,
            &request(
                &f.actor,
                1,
                Action::OpenRun {
                    task_id: task,
                    lease_id: lease.lease_id,
                    run_id: run,
                    launch_fingerprint: "b".repeat(64),
                    capabilities: AdapterCapabilities {
                        lifecycle: true,
                        tool_outcomes: true,
                        interrupt: true,
                        mutation_gating: false,
                    },
                },
            ),
        )
        .unwrap();
    let effect = Id::new_v4();
    for observation in [
        RunObservation::Spawned { pid: 123 },
        RunObservation::ToolStarted {
            source_sequence: 2,
            effect_id: effect,
            class: EffectClass::ExternalNonIdempotent,
        },
        RunObservation::TreeStopped,
    ] {
        let req = request(
            &f.actor,
            1,
            Action::ObserveRun {
                task_id: task,
                lease_id: lease.lease_id,
                run_id: run,
                observation,
            },
        );
        let first = f.store.execute(&f.actor, &req).unwrap();
        assert_eq!(f.store.execute(&f.actor, &req).unwrap(), first);
    }
    let reopened = Store::open(&f.path, f.clock.clone()).unwrap();
    let record = reopened.run(&f.actor, task, run).unwrap();
    assert_eq!(record.effects[0].state, EffectState::Uncertain);
    assert!(record.event_gap);
    assert_eq!(
        reopened
            .execution_events(&f.actor, task, run)
            .unwrap()
            .len(),
        3
    );
    let outsider = f.store.create_local_actor().unwrap();
    assert_eq!(
        reopened.run(&outsider, task, run).unwrap_err().code,
        ErrorCode::Forbidden
    );
    let stale = request(
        &f.actor,
        0,
        Action::ObserveRun {
            task_id: task,
            lease_id: lease.lease_id,
            run_id: run,
            observation: RunObservation::SupervisorLost,
        },
    );
    assert_eq!(
        code(f.store.execute(&f.actor, &stale)),
        ErrorCode::StaleEpoch
    );
    drop(reopened);
    let c = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    c.execute_batch(
        "DROP TRIGGER execution_no_delete; DELETE FROM execution_events WHERE sequence=3;",
    )
    .unwrap();
    assert_eq!(
        f.store.run(&f.actor, task, run).unwrap_err().code,
        ErrorCode::CorruptStore
    );
}

// M3-S04; FR-12/15: old schema refuses without silent migration.
#[test]
fn m3_schema_two_is_preserved_and_expired_launch_permit_is_refused() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let req = request(
        &f.actor,
        1,
        Action::OpenRun {
            task_id: task,
            lease_id: lease.lease_id,
            run_id: Id::new_v4(),
            launch_fingerprint: "d".repeat(64),
            capabilities: AdapterCapabilities {
                lifecycle: false,
                tool_outcomes: false,
                interrupt: false,
                mutation_gating: false,
            },
        },
    );
    let (_, permit) = f.store.prepare_launch(&f.actor, &req).unwrap();
    f.clock.0.store(1100, Ordering::SeqCst);
    assert_eq!(
        f.store
            .validate_launch(&f.actor, &permit.unwrap())
            .unwrap_err()
            .code,
        ErrorCode::Blocked
    );
    let c = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    c.pragma_update(None, "user_version", 2).unwrap();
    assert_eq!(
        Store::open(&f.path, f.clock.clone()).err().unwrap().code,
        ErrorCode::UnsupportedSchema
    );
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        2
    );
}

// M1-001; FR-01, INV-05: enrollment must not write repository content/index.
#[test]
fn m1_001_enrollment_is_idempotent_and_preserves_git_state() {
    let mut f = Fixture::new();
    std::fs::write(f.repo.path().join("sentinel"), [0, 255, 13, 10]).unwrap();
    f.repo.git(&["add", "sentinel"]).unwrap();
    let index_before = std::fs::read(f.repo.path().join(".git/index")).unwrap();
    std::fs::write(f.repo.path().join("sentinel"), [0, 255, 13, 10, 42]).unwrap();
    let before = f
        .repo
        .git(&["status", "--porcelain=v1", "--untracked-files=all"])
        .unwrap();
    let req = request(
        &f.actor,
        0,
        Action::EnrollRepository {
            repository_id: Id::new_v4(),
            root: f.repo.path().to_str().unwrap().into(),
        },
    );
    let reply = f.store.execute(&f.actor, &req).unwrap();
    assert_eq!(reply, Reply::Repository(f.repo_id));
    assert_eq!(f.store.execute(&f.actor, &req).unwrap(), reply);
    assert_eq!(
        f.repo
            .git(&["status", "--porcelain=v1", "--untracked-files=all"])
            .unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(f.repo.path().join("sentinel")).unwrap(),
        [0, 255, 13, 10, 42]
    );
    assert_eq!(
        std::fs::read(f.repo.path().join(".git/index")).unwrap(),
        index_before
    );
    let repo_state = f.repo.path().join("forbidden-state");
    assert_eq!(
        Store::create(&repo_state, f.clock.clone())
            .err()
            .unwrap()
            .code,
        ErrorCode::InvalidInput
    );
    assert!(!repo_state.exists());
}

// T-001-M1; FR-07, INV-01: concurrent authorized claims have one winner.
#[test]
fn t001_m1_concurrent_claims_have_one_owner() {
    let mut f = Fixture::new();
    let task = f.task();
    let barrier = Arc::new(Barrier::new(16));
    let mut clients = Vec::new();
    for _ in 0..16 {
        let actor = f.store.create_local_actor().unwrap();
        f.store
            .execute(
                &f.actor,
                &request(
                    &f.actor,
                    0,
                    Action::GrantAccess {
                        task_id: task,
                        actor_id: actor.id(),
                    },
                ),
            )
            .unwrap();
        let path = f.path.clone();
        let clock = f.clock.clone();
        let barrier = barrier.clone();
        let mut store = Store::open(&path, clock).unwrap();
        clients.push(std::thread::spawn(move || {
            barrier.wait();
            store.execute(
                &actor,
                &request(
                    &actor,
                    0,
                    Action::ClaimTask {
                        task_id: task,
                        lease_ms: 100,
                    },
                ),
            )
        }));
    }
    let results: Vec<_> = clients.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results
        .iter()
        .filter_map(|r| r.as_ref().err())
        .all(|e| e.code == ErrorCode::StaleEpoch));
    let task = f.store.task(&f.actor, task).unwrap();
    assert_eq!(task.epoch, 1);
    assert_eq!(task.state, TaskState::Reserved);
    assert!(task.owner_id.is_some());
}

// T-002-M1; FR-07, INV-01: stale metadata mutations reject and audit.
#[test]
fn t002_m1_stale_epochs_reject_and_audit_after_reclaim() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let cancel = request(
        &f.actor,
        1,
        Action::CancelReservation {
            task_id: task,
            lease_id: lease.lease_id,
        },
    );
    f.store.execute(&f.actor, &cancel).unwrap();
    let next = f.claim(task, 1);
    assert_eq!(next.epoch, 2);
    for action in [
        Action::Heartbeat {
            task_id: task,
            lease_id: lease.lease_id,
            lease_ms: 100,
        },
        Action::RecordIntent {
            task_id: task,
            lease_id: lease.lease_id,
            summary: "stale intent".into(),
        },
        Action::CancelReservation {
            task_id: task,
            lease_id: lease.lease_id,
        },
    ] {
        assert_eq!(
            code(f.store.execute(&f.actor, &request(&f.actor, 1, action))),
            ErrorCode::StaleEpoch
        );
    }
    assert_eq!(
        f.store
            .audit(&f.actor, task)
            .unwrap()
            .iter()
            .filter(|a| a.reason == Some(ErrorCode::StaleEpoch))
            .count(),
        3
    );
    assert_eq!(
        f.store.task(&f.actor, task).unwrap().owner_id,
        Some(f.actor.id())
    );
}

// T-008-M1; FR-04/08, INV-07: lost replies persist; changed keys cannot mutate.
#[test]
fn t008_m1_retries_survive_reopen_and_bind_full_request() {
    let mut f = Fixture::new();
    let task = f.task();
    let req = request(
        &f.actor,
        0,
        Action::ClaimTask {
            task_id: task,
            lease_ms: 100,
        },
    );
    let first = f.store.execute(&f.actor, &req).unwrap();
    let mut reopened = Store::open(&f.path, f.clock.clone()).unwrap();
    assert_eq!(reopened.execute(&f.actor, &req).unwrap(), first);
    let mut changed = req.clone();
    changed.expected_epoch = 1;
    assert_eq!(
        code(reopened.execute(&f.actor, &changed)),
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(reopened.task(&f.actor, task).unwrap().epoch, 1);
    assert_eq!(
        reopened
            .events(&f.actor, task)
            .unwrap()
            .iter()
            .filter(|e| e.kind == EventKind::Claimed)
            .count(),
        1
    );
}

// M1-003; FR-07, INV-06: expiry never clears ownership or enables takeover.
#[test]
fn m1_003_expiry_is_sticky_suspect_and_never_regrants() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    f.clock.0.store(1_100, Ordering::SeqCst);
    let beat = request(
        &f.actor,
        1,
        Action::Heartbeat {
            task_id: task,
            lease_id: lease.lease_id,
            lease_ms: 100,
        },
    );
    assert_eq!(code(f.store.execute(&f.actor, &beat)), ErrorCode::Blocked);
    assert_eq!(
        f.store.task(&f.actor, task).unwrap().state,
        TaskState::Suspect
    );
    f.clock.0.store(1_000, Ordering::SeqCst);
    assert_eq!(
        code(f.store.execute(
            &f.actor,
            &request(
                &f.actor,
                1,
                Action::ClaimTask {
                    task_id: task,
                    lease_ms: 100
                }
            )
        )),
        ErrorCode::Blocked
    );
    assert_eq!(
        code(f.store.execute(
            &f.actor,
            &request(
                &f.actor,
                1,
                Action::Heartbeat {
                    task_id: task,
                    lease_id: lease.lease_id,
                    lease_ms: 100
                }
            )
        )),
        ErrorCode::Blocked
    );
    assert_eq!(
        f.store.task(&f.actor, task).unwrap().owner_id,
        Some(f.actor.id())
    );
}

// M1-002; FR-02/04/07, INV-01/09: IDs alone cannot authenticate or authorize.
#[test]
fn m1_002_authentication_acl_and_validation_fail_closed() {
    let mut f = Fixture::new();
    let task = f.task();
    let other = f.store.create_local_actor().unwrap();
    let req = request(
        &f.actor,
        0,
        Action::ClaimTask {
            task_id: task,
            lease_ms: 100,
        },
    );
    assert_eq!(code(f.store.execute(&other, &req)), ErrorCode::Unauthorized);
    assert_eq!(
        code(
            f.store
                .execute(&other, &request(&other, 0, req.action.clone()))
        ),
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.store.task(&other, task).unwrap_err().code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.store.events(&other, task).unwrap_err().code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        code(f.store.execute(
            &f.actor,
            &request(
                &f.actor,
                0,
                Action::ClaimTask {
                    task_id: task,
                    lease_ms: 0
                }
            )
        )),
        ErrorCode::InvalidInput
    );
    let lease = f.claim(task, 0);
    f.store
        .execute(
            &f.actor,
            &request(
                &f.actor,
                1,
                Action::RecordIntent {
                    task_id: task,
                    lease_id: lease.lease_id,
                    summary: "agent says a test should pass".into(),
                },
            ),
        )
        .unwrap();
    let events = f.store.events(&f.actor, task).unwrap();
    assert_eq!(events.last().unwrap().origin, EventOrigin::AgentReported);
    assert!(events.last().unwrap().evidence_hash.is_none());
    assert_eq!(
        events.iter().map(|e| e.sequence).collect::<Vec<_>>(),
        (1..=events.len() as u64).collect::<Vec<_>>()
    );
}

// M1-005; FR-04, INV-09: append-only SQL and sequence gaps fail closed.
#[test]
fn m1_005_journal_is_append_only_and_detects_sequence_gaps() {
    let mut f = Fixture::new();
    let task = f.task();
    let connection = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    for (table, column) in [
        ("events", "record"),
        ("operations", "result"),
        ("audit", "record"),
    ] {
        assert!(connection
            .execute(&format!("DELETE FROM {table}"), [])
            .is_err());
        assert!(connection
            .execute(&format!("UPDATE {table} SET {column}='{{}}'"), [])
            .is_err());
    }
    connection
        .execute_batch("DROP TRIGGER events_no_update; UPDATE events SET sequence=2;")
        .unwrap();
    assert_eq!(
        f.store.events(&f.actor, task).unwrap_err().code,
        ErrorCode::CorruptStore
    );
    assert_eq!(
        code(f.store.execute(
            &f.actor,
            &request(
                &f.actor,
                0,
                Action::ClaimTask {
                    task_id: task,
                    lease_ms: 100
                }
            )
        )),
        ErrorCode::CorruptStore
    );
}

// M1-009; FR-04, INV-09: loss of the journal tail must not look complete.
#[test]
fn m1_009_deleted_journal_tail_blocks_mutation() {
    let mut f = Fixture::new();
    let task = f.task();
    f.claim(task, 0);
    let connection = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    connection
        .execute_batch("DROP TRIGGER events_no_delete; DELETE FROM events WHERE sequence=2;")
        .unwrap();
    assert_eq!(
        f.store.task(&f.actor, task).unwrap_err().code,
        ErrorCode::CorruptStore
    );
}

// M1-006; FR-04/07/08, INV-01/07: failed journal writes roll back lease and key.
#[test]
fn m1_006_failed_journal_write_rolls_back_whole_mutation() {
    let mut f = Fixture::new();
    let task = f.task();
    let connection = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT, 'injected storage failure'); END;").unwrap();
    let req = request(
        &f.actor,
        0,
        Action::ClaimTask {
            task_id: task,
            lease_ms: 100,
        },
    );
    assert_eq!(
        code(f.store.execute(&f.actor, &req)),
        ErrorCode::StoreFailure
    );
    assert_eq!(f.store.task(&f.actor, task).unwrap().epoch, 0);
    assert_eq!(f.store.events(&f.actor, task).unwrap().len(), 1);
    connection
        .execute_batch("DROP TRIGGER fail_event;")
        .unwrap();
    assert!(matches!(
        f.store.execute(&f.actor, &req).unwrap(),
        Reply::Lease(_)
    ));
}

// M1-007; FR-04, NFR-08: reject future/foreign DBs without migrating or clobbering.
#[test]
fn m1_007_future_database_is_refused_without_rewrite() {
    let f = Fixture::new();
    let connection = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    connection.pragma_update(None, "user_version", 999).unwrap();
    assert_eq!(
        Store::open(&f.path, f.clock.clone()).err().unwrap().code,
        ErrorCode::UnsupportedSchema
    );
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 999);
    assert_eq!(
        Store::create(&f.path, f.clock.clone()).err().unwrap().code,
        ErrorCode::Io
    );
    assert_eq!(
        connection
            .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        999
    );
}

// T-008-M1; FR-08, INV-07: all implemented actions and failures retain replies.
#[test]
fn t008_m1_all_mutations_and_denials_are_idempotent() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let other = f.store.create_local_actor().unwrap();
    let mut historical = Vec::new();
    for action in [
        Action::GrantAccess {
            task_id: task,
            actor_id: other.id(),
        },
        Action::Heartbeat {
            task_id: task,
            lease_id: lease.lease_id,
            lease_ms: 150,
        },
        Action::RecordIntent {
            task_id: task,
            lease_id: lease.lease_id,
            summary: "agent-reported next step".into(),
        },
        Action::CancelReservation {
            task_id: task,
            lease_id: lease.lease_id,
        },
    ] {
        let req = request(&f.actor, 1, action);
        let first = f.store.execute(&f.actor, &req).unwrap();
        let before = f.store.events(&f.actor, task).unwrap();
        f.clock.0.fetch_add(1, Ordering::SeqCst);
        assert_eq!(f.store.execute(&f.actor, &req).unwrap(), first);
        assert_eq!(f.store.events(&f.actor, task).unwrap(), before);
        historical.push((req, first));
    }
    let stale = request(
        &f.actor,
        0,
        Action::ClaimTask {
            task_id: task,
            lease_ms: 100,
        },
    );
    let failure = f.store.execute(&f.actor, &stale).unwrap_err();
    f.claim(task, 1);
    assert_eq!(f.store.execute(&f.actor, &stale).unwrap_err(), failure);
    let current = f.store.task(&f.actor, task).unwrap();
    for (req, reply) in historical {
        assert_eq!(f.store.execute(&f.actor, &req).unwrap(), reply);
        assert_eq!(f.store.task(&f.actor, task).unwrap(), current);
    }
}

// M1-010; FR-01, INV-05: alternate paths identify the same repository.
#[test]
fn m1_010_repository_aliases_are_canonicalized() {
    let mut f = Fixture::new();
    let child = f.repo.path().join("nested");
    std::fs::create_dir(&child).unwrap();
    let mut aliases = vec![
        child.to_str().unwrap().to_owned(),
        f.repo.path().join(".").to_str().unwrap().to_owned(),
    ];
    #[cfg(windows)]
    aliases.push(f.repo.path().to_str().unwrap().to_ascii_uppercase());
    #[cfg(not(windows))]
    let _ = &mut aliases;
    for root in aliases {
        assert_eq!(
            f.store
                .execute(
                    &f.actor,
                    &request(
                        &f.actor,
                        0,
                        Action::EnrollRepository {
                            repository_id: Id::new_v4(),
                            root
                        }
                    )
                )
                .unwrap(),
            Reply::Repository(f.repo_id)
        );
    }
}

// M1-011; FR-04/07, INV-01/09: readers never mix cursors from different commits.
#[test]
fn m1_011_parallel_readers_observe_consistent_journal_snapshots() {
    let mut f = Fixture::new();
    let task = f.task();
    let lease = f.claim(task, 0);
    let actor = f.actor.clone();
    let barrier = Arc::new(Barrier::new(2));
    let ready = barrier.clone();
    let mut writer = Store::open(&f.path, f.clock.clone()).unwrap();
    let writes = std::thread::spawn(move || {
        ready.wait();
        for _ in 0..100 {
            writer
                .execute(
                    &actor,
                    &request(
                        &actor,
                        1,
                        Action::Heartbeat {
                            task_id: task,
                            lease_id: lease.lease_id,
                            lease_ms: 100,
                        },
                    ),
                )
                .unwrap();
        }
    });
    barrier.wait();
    for _ in 0..100 {
        let snapshot = f.store.task(&f.actor, task).unwrap();
        assert_eq!(snapshot.epoch, 1);
        let events = f.store.events(&f.actor, task).unwrap();
        assert!(events.len() >= 2);
    }
    writes.join().unwrap();
    assert_eq!(
        f.store.task(&f.actor, task).unwrap().last_event_sequence,
        102
    );
}

// T-008-M1; FR-08, INV-07: simultaneous retries produce one journal event/grant.
#[test]
fn t008_m1_concurrent_duplicate_claims_return_identical_reply() {
    let mut f = Fixture::new();
    let task = f.task();
    let req = request(
        &f.actor,
        0,
        Action::ClaimTask {
            task_id: task,
            lease_ms: 100,
        },
    );
    let barrier = Arc::new(Barrier::new(8));
    let mut clients = Vec::new();
    for _ in 0..8 {
        let path = f.path.clone();
        let actor = f.actor.clone();
        let req = req.clone();
        let clock = f.clock.clone();
        let barrier = barrier.clone();
        let mut store = Store::open(&path, clock).unwrap();
        clients.push(std::thread::spawn(move || {
            barrier.wait();
            store.execute(&actor, &req).unwrap()
        }));
    }
    let replies: Vec<_> = clients.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(replies.iter().all(|reply| reply == &replies[0]));
    assert_eq!(f.store.events(&f.actor, task).unwrap().len(), 2);
}

fn seed_checkpoint_repo(f: &Fixture) {
    std::fs::write(f.repo.path().join("captured.txt"), b"captured state\n").unwrap();
    f.repo.git(&["add", "captured.txt"]).unwrap();
    f.repo.git(&["commit", "-m", "checkpoint fixture"]).unwrap();
}
fn capture_request(f: &Fixture, task: Id, lease: &Lease, checkpoint: Id) -> Request {
    request(
        &f.actor,
        lease.epoch,
        Action::CaptureCheckpoint {
            task_id: task,
            lease_id: lease.lease_id,
            checkpoint_id: checkpoint,
            include_untracked: Vec::new(),
        },
    )
}

// T-004/T-005/T-008-M2; FR-04/05/09, INV-03/05/07: publish verified artifacts once.
#[test]
fn m2_store_checkpoint_and_restore_retries_revalidate_artifacts() {
    let mut f = Fixture::new();
    seed_checkpoint_repo(&f);
    let task = f.task();
    let lease = f.claim(task, 0);
    let id = Id::new_v4();
    let req = capture_request(&f, task, &lease, id);
    let record = match f.store.execute(&f.actor, &req).unwrap() {
        Reply::Checkpoint(c) => c,
        _ => panic!("expected checkpoint"),
    };
    assert_eq!(record.quality, RecoveryQuality::Blocked);
    assert_eq!(record.binding.event_cursor, 2);
    assert_eq!(f.store.checkpoint(&f.actor, task, id).unwrap(), record);
    assert_eq!(
        f.store.execute(&f.actor, &req).unwrap(),
        Reply::Checkpoint(record.clone())
    );
    assert_eq!(
        f.store
            .events(&f.actor, task)
            .unwrap()
            .last()
            .unwrap()
            .evidence_hash,
        Some(record.manifest_hash.clone())
    );
    std::fs::write(f.repo.path().join("captured.txt"), b"later source edit\n").unwrap();
    let restore = request(
        &f.actor,
        1,
        Action::RestoreCheckpoint {
            task_id: task,
            lease_id: lease.lease_id,
            checkpoint_id: id,
            restore_id: Id::new_v4(),
        },
    );
    let reply = f.store.execute(&f.actor, &restore).unwrap();
    assert_eq!(f.store.execute(&f.actor, &restore).unwrap(), reply);
    let report = match reply {
        Reply::Restored(r) => r,
        _ => panic!("expected restore"),
    };
    assert_eq!(
        std::fs::read(std::path::Path::new(&report.worktree).join("captured.txt")).unwrap(),
        b"captured state\n"
    );
    std::fs::write(
        std::path::Path::new(&report.worktree).join("captured.txt"),
        b"receiver edit",
    )
    .unwrap();
    assert_eq!(
        code(f.store.execute(&f.actor, &restore)),
        ErrorCode::IntegrityFailure
    );
    assert_eq!(
        std::fs::read(f.repo.path().join("captured.txt")).unwrap(),
        b"later source edit\n"
    );
    assert!(f
        .store
        .audit(&f.actor, task)
        .unwrap()
        .iter()
        .any(|a| a.reason == Some(ErrorCode::IntegrityFailure)));
    let state = f.path.clone();
    drop(f);
    assert!(!state.exists());
}

// T-004-M2; FR-04/05, INV-03: failed SQL publication leaves only orphan artifacts.
#[test]
fn m2_store_failed_publication_never_acknowledges_root() {
    let mut f = Fixture::new();
    seed_checkpoint_repo(&f);
    let task = f.task();
    let lease = f.claim(task, 0);
    let id = Id::new_v4();
    let req = capture_request(&f, task, &lease, id);
    let c = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    c.execute_batch("CREATE TRIGGER fail_checkpoint BEFORE INSERT ON checkpoints BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert_eq!(
        code(f.store.execute(&f.actor, &req)),
        ErrorCode::StoreFailure
    );
    assert_eq!(
        f.store.checkpoint(&f.actor, task, id).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(f.store.task(&f.actor, task).unwrap().last_event_sequence, 2);
    c.execute_batch("DROP TRIGGER fail_checkpoint;").unwrap();
    assert!(matches!(
        f.store.execute(&f.actor, &req).unwrap(),
        Reply::Checkpoint(_)
    ));
}

// T-002/T-004-M2; FR-05/07, INV-01/03: stale publish paths reject before capture.
#[test]
fn m2_store_stale_checkpoint_claim_and_missing_artifacts_block() {
    let mut f = Fixture::new();
    seed_checkpoint_repo(&f);
    let task = f.task();
    let lease = f.claim(task, 0);
    let mut stale = capture_request(&f, task, &lease, Id::new_v4());
    stale.expected_epoch = 0;
    assert_eq!(
        code(f.store.execute(&f.actor, &stale)),
        ErrorCode::StaleEpoch
    );
    assert_eq!(
        std::fs::read_dir(f.path.join("checkpoints/manifests"))
            .unwrap()
            .count(),
        0
    );
    let id = Id::new_v4();
    let req = capture_request(&f, task, &lease, id);
    let record = match f.store.execute(&f.actor, &req).unwrap() {
        Reply::Checkpoint(r) => r,
        _ => panic!(),
    };
    let vault = baton_checkpoint::Vault::open(&f.path.join("checkpoints")).unwrap();
    let manifest = vault.manifest(&record.manifest_hash).unwrap();
    std::fs::remove_file(
        f.path
            .join("checkpoints/blobs")
            .join(&manifest.files[0].index.as_ref().unwrap().blob.hash),
    )
    .unwrap();
    assert_eq!(
        code(f.store.execute(&f.actor, &req)),
        ErrorCode::IntegrityFailure
    );
    assert_eq!(
        f.store.checkpoint(&f.actor, task, id).unwrap_err().code,
        ErrorCode::IntegrityFailure
    );
}

// M2-DB; NFR-08: the new database schema does not silently rewrite M1 stores.
#[test]
fn m2_store_schema_one_requires_explicit_migration() {
    let f = Fixture::new();
    let c = rusqlite::Connection::open(f.path.join("baton.sqlite3")).unwrap();
    c.pragma_update(None, "user_version", 1).unwrap();
    assert_eq!(
        Store::open(&f.path, f.clock.clone()).err().unwrap().code,
        ErrorCode::UnsupportedSchema
    );
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        1
    );
}

// M2-GC; FR-05/16, INV-03: every task and parent checkpoint remains a live root.
#[test]
fn m2_store_orphan_collection_retains_all_tasks_and_parent_roots() {
    let mut f = Fixture::new();
    seed_checkpoint_repo(&f);
    let first = f.task();
    let a = f.claim(first, 0);
    let first_id = Id::new_v4();
    let req = capture_request(&f, first, &a, first_id);
    f.store.execute(&f.actor, &req).unwrap();
    std::fs::write(f.repo.path().join("captured.txt"), b"second checkpoint\n").unwrap();
    let second_id = Id::new_v4();
    let req = capture_request(&f, first, &a, second_id);
    let second = match f.store.execute(&f.actor, &req).unwrap() {
        Reply::Checkpoint(c) => c,
        _ => panic!(),
    };
    assert_eq!(second.binding.parent_id, Some(first_id));
    let other = f.task();
    let b = f.claim(other, 0);
    let other_id = Id::new_v4();
    let req = capture_request(&f, other, &b, other_id);
    f.store.execute(&f.actor, &req).unwrap();
    let gc = request(
        &f.actor,
        1,
        Action::CollectOrphans {
            task_id: first,
            lease_id: a.lease_id,
        },
    );
    assert_eq!(
        f.store.execute(&f.actor, &gc).unwrap(),
        Reply::OrphansCollected(0)
    );
    assert_eq!(
        f.store.execute(&f.actor, &gc).unwrap(),
        Reply::OrphansCollected(0)
    );
    for (task, id) in [(first, first_id), (first, second_id), (other, other_id)] {
        f.store.checkpoint(&f.actor, task, id).unwrap();
    }
}

// M2-EXPIRY; FR-05/07, INV-01/03/06: expiry during capture cannot publish a root.
#[test]
fn m2_store_expiry_during_capture_preserves_owner_and_blocks_publication() {
    struct ExpiringClock(std::sync::atomic::AtomicUsize);
    impl Clock for ExpiringClock {
        fn now_ms(&self) -> Result<i64, Diagnostic> {
            Ok(if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                1_000
            } else {
                1_100
            })
        }
    }
    let mut f = Fixture::new();
    seed_checkpoint_repo(&f);
    let task = f.task();
    let lease = f.claim(task, 0);
    let id = Id::new_v4();
    let mut store = Store::open(
        &f.path,
        Arc::new(ExpiringClock(std::sync::atomic::AtomicUsize::new(0))),
    )
    .unwrap();
    assert_eq!(
        code(store.execute(&f.actor, &capture_request(&f, task, &lease, id))),
        ErrorCode::Blocked
    );
    assert_eq!(
        store.checkpoint(&f.actor, task, id).unwrap_err().code,
        ErrorCode::NotFound
    );
    let current = store.task(&f.actor, task).unwrap();
    assert_eq!(current.state, TaskState::Suspect);
    assert_eq!(current.owner_id, Some(f.actor.id()));
    assert_eq!(current.epoch, 1);
}
