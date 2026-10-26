use baton_protocol::*;
use baton_store::{Actor, Clock, Store};
use baton_supervisor::{LaunchSpec, Session, TreeStatus};
use baton_test_support::GitFixture;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

// M4-HS05; INV-02: a source run cannot acknowledge receiver readiness.
#[test]
fn source_session_cannot_acknowledge_receiver_readiness() {
    let mut f = Fixture::new();
    let mut session = f.launch("wait");
    let hello = ReceiverSession {
        schema_version: SchemaVersion::V0_1_0,
        binding: session.binding().clone(),
        session_id: Id::new_v4(),
        adapter: AgentMetadata {
            name: "fixture".into(),
            version: "1".into(),
        },
    };
    assert_eq!(
        session
            .acknowledge_receiver(&mut f.store, &f.actor, Id::new_v4(), &hello)
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    session
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
}

// M4-NP01; FR-08/12, INV-02/06/09: persisted drain comes from a bound live scope.
#[cfg(windows)]
#[test]
fn m4_native_drain_is_bound_to_real_scope_and_survives_restart() {
    let mut f = Fixture::new();
    let mut session = f.launch("parent");
    until(|| f.temp.path().join("writer-ready").exists());
    let binding = session.binding().clone();
    assert_eq!(
        f.store
            .native_drain(&f.actor, f.task, binding.run_id)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    session
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    let proof = f
        .store
        .native_drain(&f.actor, f.task, binding.run_id)
        .unwrap();
    assert_eq!(proof.scope.binding, binding);
    assert!(!proof.scope.scope_id.is_nil());
    assert_eq!(proof.observed_source_cursor, 0);
    assert!(proof.execution_sequence >= 2);
    assert!(f
        .store
        .run(&f.actor, f.task, binding.run_id)
        .unwrap()
        .blockers()
        .contains(&BoundaryReason::AdapterUnvalidated));
    f._repo
        .git(&["commit", "--allow-empty", "-m", "synthetic checkpoint"])
        .unwrap();
    let transfer_id = Id::new_v4();
    f.store
        .execute(
            &f.actor,
            &req(
                &f.actor,
                Action::RequestTransfer {
                    task_id: f.task,
                    lease_id: f.lease.lease_id,
                    transfer_id,
                    receiver_id: f.actor.id(),
                },
            ),
        )
        .unwrap();
    let checkpoint_id = Id::new_v4();
    f.store
        .execute(
            &f.actor,
            &req(
                &f.actor,
                Action::CaptureCheckpoint {
                    task_id: f.task,
                    lease_id: f.lease.lease_id,
                    checkpoint_id,
                    include_untracked: vec![],
                },
            ),
        )
        .unwrap();
    let preflight = req(
        &f.actor,
        Action::PreflightTransfer {
            task_id: f.task,
            lease_id: f.lease.lease_id,
            transfer_id,
            checkpoint_id,
        },
    );
    let Reply::Transfer(report) = f.store.execute(&f.actor, &preflight).unwrap() else {
        panic!()
    };
    assert_eq!(report.source_drain.as_ref(), Some(&proof));
    assert_eq!(report.state, TransferState::Blocked);
    assert!(report
        .reasons
        .contains(&TransferReason::SourceFenceUnavailable));
    assert_eq!(
        f.store.execute(&f.actor, &preflight).unwrap(),
        Reply::Transfer(report)
    );
    drop(session);
    f.store = Store::open(&f.temp.path().join("state"), f.clock.clone()).unwrap();
    assert_eq!(
        f.store
            .native_drain(&f.actor, f.task, binding.run_id)
            .unwrap(),
        proof
    );
}

// M4-NP02; INV-02/06: an empty unrelated job cannot attest to another live run.
#[cfg(windows)]
#[test]
fn m4_native_drain_rejects_proof_from_another_scope() {
    use baton_platform::process::ProcessTree;
    let mut f = Fixture::new();
    let mut session = f.launch("wait");
    let binding = session.binding().clone();
    let spec = LaunchSpec::new(
        Path::new(env!("CARGO_BIN_EXE_baton-fake-agent")),
        f.temp.path(),
        ["wait".into(), f.temp.path().as_os_str().to_owned()],
    )
    .unwrap();
    let mut other = ProcessTree::spawn(spec).unwrap();
    assert_eq!(other.drain_proof().err().unwrap().code, ErrorCode::Blocked);
    other.terminate().unwrap();
    until(|| other.status().unwrap() == TreeStatus::Empty);
    let proof = other.drain_proof().unwrap();
    assert_eq!(
        f.store
            .record_native_drain(&f.actor, &binding, &proof, Id::new_v4())
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(session.tree_status().unwrap(), TreeStatus::Active);
    assert_eq!(
        f.store
            .native_drain(&f.actor, f.task, binding.run_id)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    session
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
}

struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn now_ms(&self) -> Result<i64, Diagnostic> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct Fixture {
    store: Store,
    actor: Actor,
    task: Id,
    lease: Lease,
    clock: Arc<TestClock>,
    _repo: GitFixture,
    temp: tempfile::TempDir,
}
fn req(actor: &Actor, action: Action) -> Request {
    Request {
        schema_version: SchemaVersion::V0_1_0,
        operation_id: Id::new_v4(),
        actor_id: actor.id(),
        expected_epoch: 1,
        action,
    }
}
impl Fixture {
    fn new() -> Self {
        Self::with_temp(tempfile::tempdir().unwrap())
    }
    fn with_temp(temp: tempfile::TempDir) -> Self {
        Self::with_scope(temp, "temp")
    }
    fn with_scope(temp: tempfile::TempDir, scope: &str) -> Self {
        let clock = Arc::new(TestClock(AtomicI64::new(1000)));
        let mut store = Store::create(&temp.path().join("state"), clock.clone()).unwrap();
        let actor = store.create_local_actor().unwrap();
        let repo = GitFixture::new().unwrap();
        let repository_id = Id::new_v4();
        let mut request = req(
            &actor,
            Action::EnrollRepository {
                repository_id,
                root: repo.path().to_str().unwrap().into(),
            },
        );
        request.expected_epoch = 0;
        store.execute(&actor, &request).unwrap();
        let task = Id::new_v4();
        request = req(
            &actor,
            Action::CreateTask {
                task_id: task,
                repository_id,
                goal: "synthetic process test".into(),
                scope: scope.into(),
                agent: AgentMetadata {
                    name: "fake-agent".into(),
                    version: "0.1".into(),
                },
            },
        );
        request.expected_epoch = 0;
        store.execute(&actor, &request).unwrap();
        request = req(
            &actor,
            Action::ClaimTask {
                task_id: task,
                lease_ms: 300_000,
            },
        );
        request.expected_epoch = 0;
        let Reply::Lease(lease) = store.execute(&actor, &request).unwrap() else {
            panic!()
        };
        Self {
            store,
            actor,
            task,
            lease,
            clock,
            _repo: repo,
            temp,
        }
    }
    fn launch(&mut self, mode: &str) -> Session {
        let cwd = self.temp.path().to_path_buf();
        self.launch_in(mode, &cwd)
    }
    fn launch_in(&mut self, mode: &str, cwd: &Path) -> Session {
        let spec = LaunchSpec::new(
            Path::new(env!("CARGO_BIN_EXE_baton-fake-agent")),
            cwd,
            [mode.into(), self.temp.path().as_os_str().to_owned()],
        )
        .unwrap();
        let request = req(
            &self.actor,
            Action::OpenRun {
                task_id: self.task,
                lease_id: self.lease.lease_id,
                run_id: Id::new_v4(),
                launch_fingerprint: spec.fingerprint(),
                capabilities: AdapterCapabilities {
                    lifecycle: true,
                    tool_outcomes: true,
                    interrupt: true,
                    mutation_gating: false,
                },
            },
        );
        let (_, permit) = self.store.prepare_launch(&self.actor, &request).unwrap();
        assert!(self
            .store
            .prepare_launch(&self.actor, &request)
            .unwrap()
            .1
            .is_none());
        Session::launch(&mut self.store, &self.actor, permit.unwrap(), spec).unwrap()
    }
}
fn until(mut condition: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < end, "observed condition timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

// M5-W04; FR-03/06/14, INV-04/05/07/09: concrete writes require fresh native admission.
#[test]
fn host_file_creation_is_audited_and_never_replayed() {
    let mut f = Fixture::with_scope(tempfile::tempdir().unwrap(), ".");
    let cwd = f._repo.path().to_path_buf();
    let mut session = f.launch_in("wait", &cwd);
    let operation = Id::new_v4();
    session
        .create_file(&mut f.store, &f.actor, operation, "new.txt", b"evidence")
        .unwrap();
    assert_eq!(std::fs::read(cwd.join("new.txt")).unwrap(), b"evidence");
    let run = f
        .store
        .run(&f.actor, f.task, session.binding().run_id)
        .unwrap();
    assert_eq!(run.effects.len(), 1);
    assert_eq!(run.effects[0].state, EffectState::Completed);
    assert_eq!(run.effects[0].outcome, Some(ToolOutcome::Succeeded));
    assert!(f
        .store
        .execution_event(&f.actor, f.task, run.binding.run_id, operation)
        .unwrap()
        .is_some());
    std::fs::remove_file(cwd.join("new.txt")).unwrap();
    assert!(session
        .create_file(&mut f.store, &f.actor, operation, "new.txt", b"evidence")
        .is_err());
    assert!(!cwd.join("new.txt").exists());
    assert_eq!(
        session
            .create_file(&mut f.store, &f.actor, operation, "other.txt", b"changed")
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    assert!(!cwd.join("other.txt").exists());
    session
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    assert!(session
        .create_file(&mut f.store, &f.actor, Id::new_v4(), "late.txt", b"no")
        .is_err());
    assert!(!cwd.join("late.txt").exists());
}

// M5-W05; INV-05/09: ambiguous failed creation preserves work and stops the scope.
#[test]
fn host_file_creation_failure_preserves_existing_work_and_stops() {
    let mut f = Fixture::with_scope(tempfile::tempdir().unwrap(), ".");
    let cwd = f._repo.path().to_path_buf();
    std::fs::write(cwd.join("existing.txt"), b"preserve").unwrap();
    let mut session = f.launch_in("wait", &cwd);
    assert!(session
        .create_file(
            &mut f.store,
            &f.actor,
            Id::new_v4(),
            "existing.txt",
            b"overwrite"
        )
        .is_err());
    assert_eq!(
        std::fs::read(cwd.join("existing.txt")).unwrap(),
        b"preserve"
    );
    let run = f
        .store
        .run(&f.actor, f.task, session.binding().run_id)
        .unwrap();
    assert_eq!(run.state, RunState::RecoveryRequired);
    assert_eq!(run.effects[0].state, EffectState::Uncertain);
    until(|| session.root_exited().unwrap());
}

// M5-W06; FR-06/14, INV-04/06: admission failures must precede filesystem writes.
#[test]
fn host_file_creation_refuses_expiry_transfer_and_wrong_workspace() {
    for reason in ["expiry", "transfer", "workspace"] {
        let mut f = Fixture::with_scope(tempfile::tempdir().unwrap(), ".");
        let repo = f._repo.path().to_path_buf();
        let cwd = if reason == "workspace" {
            f.temp.path().to_path_buf()
        } else {
            repo.clone()
        };
        let mut session = f.launch_in("wait", &cwd);
        match reason {
            "expiry" => f.clock.0.store(301_000, Ordering::SeqCst),
            "transfer" => {
                f.store
                    .execute(
                        &f.actor,
                        &req(
                            &f.actor,
                            Action::RequestTransfer {
                                task_id: f.task,
                                lease_id: f.lease.lease_id,
                                transfer_id: Id::new_v4(),
                                receiver_id: f.actor.id(),
                            },
                        ),
                    )
                    .unwrap();
            }
            _ => (),
        }
        assert!(
            session
                .create_file(&mut f.store, &f.actor, Id::new_v4(), "denied.txt", b"no")
                .is_err(),
            "{reason}"
        );
        assert!(!cwd.join("denied.txt").exists());
        assert!(!repo.join("denied.txt").exists());
        // Expired authority can refuse the final journal even after native teardown.
        let _ = session.stop(&mut f.store, &f.actor, Duration::from_secs(5));
        until(|| session.root_exited().unwrap());
    }
}

// T-006-M3; FR-03/06, INV-06/09: a dead root is not a drained Job Object.
#[cfg(windows)]
#[test]
fn t006_parent_exit_leaves_live_child_until_tree_stop() {
    let mut f = Fixture::new();
    let mut session = f.launch("parent");
    until(|| f.temp.path().join("parent-exiting").exists());
    until(|| session.root_exited().unwrap());
    assert_eq!(session.tree_status().unwrap(), TreeStatus::Active);
    let run = session.binding().run_id;
    assert!(!f.store.run(&f.actor, f.task, run).unwrap().tree_stopped);
    session
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    assert_eq!(session.tree_status().unwrap(), TreeStatus::Empty);
    assert!(f.store.run(&f.actor, f.task, run).unwrap().tree_stopped);
}

// M3-N02; FR-06/15, INV-09: acknowledgement remains distinct from drain.
#[test]
fn interrupt_acknowledgement_is_not_a_stopped_process() {
    let mut f = Fixture::new();
    let mut session = f.launch("wait");
    until(|| f.temp.path().join("agent-ready").exists());
    session
        .request_interrupt(&mut f.store, &f.actor, || Ok(()))
        .unwrap();
    session
        .observe(
            &mut f.store,
            &f.actor,
            Id::new_v4(),
            RunObservation::InterruptAcknowledged { source_sequence: 1 },
        )
        .unwrap();
    assert_eq!(session.tree_status().unwrap(), TreeStatus::Active);
    assert!(
        !f.store
            .run(&f.actor, f.task, session.binding().run_id)
            .unwrap()
            .tree_stopped
    );
    session
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
}

// T-007-M3; FR-14, INV-04: killing local commands cannot settle remote effects.
#[test]
fn unresolved_deployment_stays_uncertain_after_process_stop() {
    let mut f = Fixture::new();
    let mut session = f.launch("deploy");
    let effect_id = Id::new_v4();
    session
        .observe(
            &mut f.store,
            &f.actor,
            Id::new_v4(),
            RunObservation::ToolStarted {
                source_sequence: 1,
                effect_id,
                class: EffectClass::ExternalNonIdempotent,
            },
        )
        .unwrap();
    std::fs::write(
        f.temp.path().join("effect-authorized"),
        b"approved synthetic effect",
    )
    .unwrap();
    until(|| f.temp.path().join("agent-ready").exists());
    session
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    let record = f
        .store
        .run(&f.actor, f.task, session.binding().run_id)
        .unwrap();
    assert_eq!(record.effects[0].state, EffectState::Uncertain);
    assert!(record
        .blockers()
        .contains(&BoundaryReason::UnresolvedEffect));
    assert_eq!(
        std::fs::read(f.temp.path().join("external-service-committed")).unwrap(),
        b"one mutation"
    );
}

// T-006-M3; FR-06/07/12, INV-06: expiry alone leaves a real writer active.
#[test]
fn lease_expiry_never_stops_or_reassigns_a_live_writer() {
    let mut f = Fixture::new();
    let mut session = f.launch("wait");
    until(|| f.temp.path().join("agent-ready").exists());
    f.clock.0.store(301_000, Ordering::SeqCst);
    let heartbeat = req(
        &f.actor,
        Action::Heartbeat {
            task_id: f.task,
            lease_id: f.lease.lease_id,
            lease_ms: 100,
        },
    );
    assert_eq!(
        f.store.execute(&f.actor, &heartbeat).unwrap_err().code,
        ErrorCode::Blocked
    );
    assert_eq!(
        f.store.task(&f.actor, f.task).unwrap().state,
        TaskState::Suspect
    );
    assert_eq!(session.tree_status().unwrap(), TreeStatus::Active);
    let claim = req(
        &f.actor,
        Action::ClaimTask {
            task_id: f.task,
            lease_ms: 100,
        },
    );
    assert_eq!(
        f.store.execute(&f.actor, &claim).unwrap_err().code,
        ErrorCode::Blocked
    );
    let _ = session.stop(&mut f.store, &f.actor, Duration::from_secs(5));
    #[cfg(windows)]
    assert_eq!(session.tree_status().unwrap(), TreeStatus::Empty);
    assert_eq!(f.store.task(&f.actor, f.task).unwrap().epoch, 1);
    assert_eq!(
        f.store
            .run(&f.actor, f.task, session.binding().run_id)
            .unwrap()
            .state,
        RunState::RecoveryRequired
    );
}

// M3-N04; FR-03/15, INV-09: adapters cannot assert process facts.
#[test]
fn adapter_cannot_forge_tree_stop_and_launch_spec_is_bound() {
    let mut f = Fixture::new();
    let mut session = f.launch("wait");
    assert_eq!(
        session
            .observe(
                &mut f.store,
                &f.actor,
                Id::new_v4(),
                RunObservation::TreeStopped
            )
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(session.tree_status().unwrap(), TreeStatus::Active);
    session
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    assert!(LaunchSpec::new(Path::new("relative-command"), f.temp.path(), []).is_err());
}

// M3-N06; FR-03/12, INV-07: a permit binds exact launch parameters, not just a run ID.
#[test]
fn changed_launch_parameters_cannot_consume_permission_for_another_command() {
    let mut f = Fixture::new();
    let program = Path::new(env!("CARGO_BIN_EXE_baton-fake-agent"));
    let expected = LaunchSpec::new(
        program,
        f.temp.path(),
        ["wait".into(), f.temp.path().as_os_str().to_owned()],
    )
    .unwrap();
    let changed = LaunchSpec::new(
        program,
        f.temp.path(),
        ["parent".into(), f.temp.path().as_os_str().to_owned()],
    )
    .unwrap();
    let run_id = Id::new_v4();
    let request = req(
        &f.actor,
        Action::OpenRun {
            task_id: f.task,
            lease_id: f.lease.lease_id,
            run_id,
            launch_fingerprint: expected.fingerprint(),
            capabilities: AdapterCapabilities {
                lifecycle: false,
                tool_outcomes: false,
                interrupt: false,
                mutation_gating: false,
            },
        },
    );
    let (_, permit) = f.store.prepare_launch(&f.actor, &request).unwrap();
    assert_eq!(
        Session::launch(&mut f.store, &f.actor, permit.unwrap(), changed)
            .err()
            .unwrap()
            .code,
        ErrorCode::InvalidInput
    );
    assert!(!f.temp.path().join("writer-ready").exists());
    let record = f.store.run(&f.actor, f.task, run_id).unwrap();
    assert_eq!(record.state, RunState::LaunchPending);
    assert!(record
        .blockers()
        .contains(&BoundaryReason::AdapterUnsupported));
    assert!(f
        .store
        .prepare_launch(&f.actor, &request)
        .unwrap()
        .1
        .is_none());
}

// M5-B04; FR-03/12/15, INV-04/07/09: changed bytes cannot use a same-path launch reservation.
#[test]
fn changed_executable_cannot_consume_original_launch_permission() {
    let mut f = Fixture::new();
    let program = f.temp.path().join("owned-program.exe");
    std::fs::write(&program, b"original program").unwrap();
    let fingerprint = LaunchSpec::new(&program, f.temp.path(), [])
        .unwrap()
        .fingerprint();
    let run_id = Id::new_v4();
    let request = req(
        &f.actor,
        Action::OpenRun {
            task_id: f.task,
            lease_id: f.lease.lease_id,
            run_id,
            launch_fingerprint: fingerprint,
            capabilities: AdapterCapabilities {
                lifecycle: false,
                tool_outcomes: false,
                interrupt: false,
                mutation_gating: false,
            },
        },
    );
    let (_, permit) = f.store.prepare_launch(&f.actor, &request).unwrap();
    std::fs::write(&program, b"changed program!").unwrap();
    let changed = LaunchSpec::new(&program, f.temp.path(), []).unwrap();
    assert!(
        matches!(Session::launch(&mut f.store, &f.actor, permit.unwrap(), changed), Err(error) if error.code == ErrorCode::InvalidInput)
    );
    assert_eq!(
        f.store.run(&f.actor, f.task, run_id).unwrap().state,
        RunState::LaunchPending
    );
    assert!(f
        .store
        .prepare_launch(&f.actor, &request)
        .unwrap()
        .1
        .is_none());
}

// M3-N07; FR-12/19: spawn failure preserves ambiguous intent and a safe diagnostic.
#[test]
fn invalid_executable_marks_recovery_required_without_retrying() {
    let mut f = Fixture::new();
    let program = f.temp.path().join("invalid.exe");
    std::fs::write(&program, b"synthetic non-executable").unwrap();
    let spec = LaunchSpec::new(&program, f.temp.path(), []).unwrap();
    let run_id = Id::new_v4();
    let request = req(
        &f.actor,
        Action::OpenRun {
            task_id: f.task,
            lease_id: f.lease.lease_id,
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
    assert_eq!(
        Session::launch(&mut f.store, &f.actor, permit.unwrap(), spec)
            .err()
            .unwrap()
            .code,
        ErrorCode::SupervisionFailure
    );
    assert_eq!(
        f.store.run(&f.actor, f.task, run_id).unwrap().state,
        RunState::RecoveryRequired
    );
    assert!(f
        .store
        .prepare_launch(&f.actor, &request)
        .unwrap()
        .1
        .is_none());
}

// M3-N05; FR-12, INV-06: OS teardown must close the owned kill-on-close job.
#[cfg(windows)]
#[test]
fn killing_supervisor_terminates_descendants_via_job_close() {
    use std::{
        fs::OpenOptions,
        os::windows::fs::OpenOptionsExt,
        process::{Command, Stdio},
    };
    let temp = tempfile::tempdir().unwrap();
    let mut owner = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_owner", "--ignored", "--nocapture"])
        .env("BATON_M3_CRASH_ROOT", temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    while !temp.path().join("owner-ready").exists() && Instant::now() < end {
        assert!(
            owner.try_wait().unwrap().is_none(),
            "owner exited before marker"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let ready = temp.path().join("owner-ready").exists();
    owner.kill().unwrap();
    owner.wait().unwrap();
    assert!(ready);
    let owned = std::fs::read_to_string(temp.path().join("owned-temp")).unwrap();
    until(|| {
        OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(Path::new(&owned).join("writer-lock"))
            .is_ok()
    });
}

#[test]
#[ignore = "subprocess helper explicitly invoked by the supervisor crash test"]
fn crash_owner() {
    let Some(root) = std::env::var_os("BATON_M3_CRASH_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let mut f = Fixture::with_temp(tempfile::tempdir_in(root).unwrap());
    let mut session = f.launch("parent");
    until(|| f.temp.path().join("writer-ready").exists());
    assert_eq!(session.tree_status().unwrap(), TreeStatus::Active);
    std::fs::write(root.join("owned-temp"), f.temp.path().to_str().unwrap()).unwrap();
    std::fs::write(root.join("owner-ready"), b"ready").unwrap();
    thread::sleep(Duration::from_secs(30));
}
