//! Closed fixture boundary assumptions test persistence, not production fencing.
use super::*;
use baton_test_support::GitFixture;

struct FixedClock;
impl Clock for FixedClock {
    fn now_ms(&self) -> Result<i64, Diagnostic> {
        Ok(1000)
    }
}
struct Fixture {
    store: Store,
    source: Actor,
    receiver: Actor,
    task: Id,
    lease: Lease,
    transfer: Id,
    stage: Id,
    restore: Id,
    repo: GitFixture,
    temp: tempfile::TempDir,
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
impl Fixture {
    fn start_receiver(
        &mut self,
    ) -> (
        RunRecord,
        baton_platform::process::ProcessTree,
        ReceiverSession,
    ) {
        launch_fixture_receiver(
            &mut self.store,
            &self.receiver,
            self.task,
            self.transfer,
            None,
        )
    }
    fn receiver_spec(&self) -> baton_platform::process::LaunchSpec {
        baton_platform::process::LaunchSpec::new(
            &std::env::current_exe().unwrap(),
            &self
                .temp
                .path()
                .join("state/checkpoints/restores")
                .join(self.restore.to_string()),
            ["--help".into()],
        )
        .unwrap()
    }
    fn activation_request(&self) -> Request {
        request(
            &self.receiver,
            2,
            Action::ActivateReceiver {
                task_id: self.task,
                transfer_id: self.transfer,
            },
        )
    }
}

fn launch_fixture_receiver(
    store: &mut Store,
    actor: &Actor,
    task: Id,
    transfer: Id,
    expected: Option<&ReceiverSession>,
) -> (
    RunRecord,
    baton_platform::process::ProcessTree,
    ReceiverSession,
) {
    let transfer_record = store.transfer(actor, task, transfer).unwrap();
    let target = PathBuf::from(
        &transfer_record
            .receiver_stage
            .unwrap()
            .report
            .unwrap()
            .worktree,
    );
    let restore = transfer_record.restore_id.unwrap();
    let spec = baton_platform::process::LaunchSpec::new(
        &std::env::current_exe().unwrap(),
        &target,
        [
            "--exact".into(),
            "engine::transfer_tests::receiver_fixture_child".into(),
            "--ignored".into(),
        ],
    )
    .unwrap();
    let (run, permit) = store
        .prepare_receiver_launch(
            actor,
            &request(
                actor,
                transfer_record.committed_epoch.unwrap(),
                Action::ActivateReceiver {
                    task_id: task,
                    transfer_id: transfer,
                },
            ),
            expected
                .map(|hello| hello.binding.run_id)
                .unwrap_or_else(Id::new_v4),
            &spec,
            receiver_capabilities(),
        )
        .unwrap();
    let challenge = target
        .parent()
        .unwrap()
        .join(format!("{restore}.challenge"));
    let expected = expected.cloned().unwrap_or_else(|| ReceiverSession {
        schema_version: SchemaVersion::V0_1_0,
        binding: run.binding.clone(),
        session_id: Id::new_v4(),
        adapter: AgentMetadata {
            name: "fixture".into(),
            version: "1".into(),
        },
    });
    assert_eq!(expected.binding, run.binding);
    fs::write(challenge, encode(&expected).unwrap()).unwrap();
    let mut tree = baton_platform::process::ProcessTree::spawn(spec).unwrap();
    store
        .record_native_spawn(actor, &permit.unwrap(), &tree, Id::new_v4())
        .unwrap();
    let response = target.parent().unwrap().join(format!("{restore}.ready"));
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while !response.exists() {
        assert!(
            !tree.root_exited().unwrap(),
            "fixture receiver exited before readiness"
        );
        assert!(
            std::time::Instant::now() < until,
            "fixture readiness timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let bytes = fs::read(response).unwrap();
    assert!(bytes.len() < 4096);
    let hello: ReceiverSession = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(hello, expected);
    (run, tree, hello)
}
impl Fixture {
    fn new(simulated: bool) -> Self {
        Self::new_with_run(simulated, false)
    }
    fn new_with_run(simulated: bool, source_run: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::create(&temp.path().join("state"), Arc::new(FixedClock)).unwrap();
        let source = store.create_local_actor().unwrap();
        let receiver = store.create_local_actor().unwrap();
        let repo = GitFixture::new().unwrap();
        fs::write(repo.path().join("file"), b"fixture source\n").unwrap();
        repo.git(&["add", "file"]).unwrap();
        repo.git(&["commit", "-m", "fixture"]).unwrap();
        let repository_id = Id::new_v4();
        let task = Id::new_v4();
        store
            .execute(
                &source,
                &request(
                    &source,
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
                &source,
                &request(
                    &source,
                    0,
                    Action::CreateTask {
                        task_id: task,
                        repository_id,
                        goal: "closed simulation".into(),
                        scope: ".".into(),
                        agent: AgentMetadata {
                            name: "fixture".into(),
                            version: "1".into(),
                        },
                    },
                ),
            )
            .unwrap();
        let Reply::Lease(lease) = store
            .execute(
                &source,
                &request(
                    &source,
                    0,
                    Action::ClaimTask {
                        task_id: task,
                        lease_ms: 300_000,
                    },
                ),
            )
            .unwrap()
        else {
            panic!()
        };
        store
            .execute(
                &source,
                &request(
                    &source,
                    1,
                    Action::GrantAccess {
                        task_id: task,
                        actor_id: receiver.id(),
                    },
                ),
            )
            .unwrap();
        if source_run {
            store
                .prepare_launch(
                    &source,
                    &request(
                        &source,
                        1,
                        Action::OpenRun {
                            task_id: task,
                            lease_id: lease.lease_id,
                            run_id: Id::new_v4(),
                            launch_fingerprint: "a".repeat(64),
                            capabilities: receiver_capabilities(),
                        },
                    ),
                )
                .unwrap();
        }
        let checkpoint_id = Id::new_v4();
        store
            .execute(
                &source,
                &request(
                    &source,
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
        let transfer = Id::new_v4();
        store
            .execute(
                &source,
                &request(
                    &source,
                    1,
                    Action::RequestTransfer {
                        task_id: task,
                        lease_id: lease.lease_id,
                        transfer_id: transfer,
                        receiver_id: receiver.id(),
                    },
                ),
            )
            .unwrap();
        if simulated {
            transfers::simulate_boundary(task);
        }
        let Reply::Transfer(prepared) = store
            .execute(
                &source,
                &request(
                    &source,
                    1,
                    Action::PreflightTransfer {
                        task_id: task,
                        lease_id: lease.lease_id,
                        transfer_id: transfer,
                        checkpoint_id,
                    },
                ),
            )
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            prepared.state,
            if simulated {
                TransferState::Prepared
            } else {
                TransferState::Blocked
            }
        );
        Self {
            store,
            source,
            receiver,
            task,
            lease,
            transfer,
            stage: Id::new_v4(),
            restore: Id::new_v4(),
            repo,
            temp,
        }
    }
    fn begin(&mut self) {
        self.store
            .execute(
                &self.receiver,
                &request(
                    &self.receiver,
                    1,
                    Action::BeginReceiverStaging {
                        task_id: self.task,
                        transfer_id: self.transfer,
                        restore_id: self.restore,
                        stage_operation_id: self.stage,
                    },
                ),
            )
            .unwrap();
    }
    fn stage_request(&self) -> Request {
        let mut r = request(
            &self.receiver,
            1,
            Action::StageReceiver {
                task_id: self.task,
                transfer_id: self.transfer,
            },
        );
        r.operation_id = self.stage;
        r
    }
    fn ready(&mut self) {
        self.begin();
        let Reply::Transfer(ready) = self
            .store
            .execute(&self.receiver, &self.stage_request())
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(ready.state, TransferState::ReadyToCommit);
    }
    fn commit_request(&self) -> Request {
        request(
            &self.receiver,
            1,
            Action::CommitTransfer {
                task_id: self.task,
                transfer_id: self.transfer,
                accept_degraded: false,
                lease_ms: 300_000,
            },
        )
    }
}

pub(super) fn receiver_capabilities() -> AdapterCapabilities {
    AdapterCapabilities {
        lifecycle: true,
        tool_outcomes: true,
        interrupt: true,
        mutation_gating: true,
    }
}

// M4-HS01; FR-08/12/13/15, INV-02/07/08/09: readiness is scoped evidence, not a PID assertion.
#[test]
fn receiver_readiness_requires_matching_live_scope_and_is_idempotent() {
    let mut f = Fixture::new(true);
    f.ready();
    let commit = f.commit_request();
    f.store.execute(&f.receiver, &commit).unwrap();
    let (run, mut tree, hello) = f.start_receiver();
    let proof = tree.running_proof().unwrap();
    let operation = Id::new_v4();
    let active = f
        .store
        .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, operation)
        .unwrap();
    assert_eq!(active.state, TransferState::Active);
    assert_eq!(active.quality, RecoveryQuality::Blocked);
    assert_eq!(
        f.store
            .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, operation)
            .unwrap(),
        active
    );
    assert_eq!(active.receiver_ready.unwrap().scope.binding, run.binding);
    assert_eq!(
        f.store.task(&f.receiver, f.task).unwrap().state,
        TaskState::Active
    );
    f.store.execute(&f.receiver, &commit).unwrap();
    assert_eq!(
        fs::read(f.receiver_spec().working_directory().join("file")).unwrap(),
        b"receiver progress\n"
    );
    assert_eq!(
        fs::read(f.repo.path().join("file")).unwrap(),
        b"fixture source\n"
    );
}

// M4-SEQ01; FR-08/09/12/13, INV-01/02/05/06/07: A -> B -> A preserves actual receiver workspaces.
#[cfg(windows)]
#[test]
fn successive_transfer_captures_current_worktree_and_advances_epoch_again() {
    let mut f = Fixture::new(true);
    fs::write(
        f.temp
            .path()
            .join("state/checkpoints/restores")
            .join(format!("{}.controlled", f.restore)),
        b"wait for tool decision",
    )
    .unwrap();
    f.ready();
    let first_commit = f.commit_request();
    let first_result = f.store.execute(&f.receiver, &first_commit).unwrap();
    let (run, mut tree, hello) = f.start_receiver();
    assert_eq!(
        fs::read(f.receiver_spec().working_directory().join("file")).unwrap(),
        b"fixture source\n"
    );
    {
        let proof = tree.running_proof().unwrap();
        f.store
            .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
            .unwrap();
    }
    let target = f.receiver_spec().working_directory().to_path_buf();
    let completed = fixture_tool(&mut f.store, &f.receiver, &run.binding, &target, &mut tree);
    assert_eq!(completed.source_cursor, 3);
    assert_eq!(completed.effects.len(), 1);
    assert_eq!(completed.effects[0].state, EffectState::Completed);
    let task = f.store.task(&f.receiver, f.task).unwrap();
    let transfer = Id::new_v4();
    let requested = request(
        &f.receiver,
        2,
        Action::RequestTransfer {
            task_id: f.task,
            lease_id: task.lease_id.unwrap(),
            transfer_id: transfer,
            receiver_id: f.source.id(),
        },
    );
    f.store.execute(&f.receiver, &requested).unwrap();
    assert_eq!(
        serde_json::to_value(f.store.task(&f.receiver, f.task).unwrap().state).unwrap(),
        "QUIESCING"
    );
    let denied = request(
        &f.receiver,
        2,
        Action::ObserveRun {
            task_id: f.task,
            lease_id: run.binding.lease_id,
            run_id: run.binding.run_id,
            observation: RunObservation::ToolReserved {
                source_sequence: 4,
                effect_id: Id::new_v4(),
                class: EffectClass::LocalReversible,
            },
        },
    );
    {
        let proof = tree.running_proof().unwrap();
        assert_eq!(
            f.store
                .prepare_tool(&f.receiver, &denied, &proof)
                .err()
                .unwrap()
                .code,
            ErrorCode::Blocked
        );
    }
    tree.terminate().unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while tree.status().unwrap() != baton_platform::process::TreeStatus::Empty {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    let proof = tree.drain_proof().unwrap();
    f.store
        .record_native_drain(&f.receiver, &run.binding, &proof, Id::new_v4())
        .unwrap();
    let checkpoint = Id::new_v4();
    let Reply::Checkpoint(captured) = f
        .store
        .execute(
            &f.receiver,
            &request(
                &f.receiver,
                2,
                Action::CaptureCheckpoint {
                    task_id: f.task,
                    lease_id: task.lease_id.unwrap(),
                    checkpoint_id: checkpoint,
                    include_untracked: vec![],
                },
            ),
        )
        .unwrap()
    else {
        panic!()
    };
    let vault = snapshots::vault(&f.store.root).unwrap();
    let manifest = vault.manifest(&captured.manifest_hash).unwrap();
    let file = manifest
        .files
        .iter()
        .find(|file| file.path == "file")
        .unwrap();
    assert_eq!(
        file.worktree.as_ref().unwrap().blob.hash,
        blake3::hash(b"receiver progress\n").to_hex().as_str()
    );
    f.store
        .execute(
            &f.receiver,
            &request(
                &f.receiver,
                2,
                Action::PreflightTransfer {
                    task_id: f.task,
                    lease_id: task.lease_id.unwrap(),
                    transfer_id: transfer,
                    checkpoint_id: checkpoint,
                },
            ),
        )
        .unwrap();
    let restore = Id::new_v4();
    let stage = Id::new_v4();
    f.store
        .execute(
            &f.source,
            &request(
                &f.source,
                2,
                Action::BeginReceiverStaging {
                    task_id: f.task,
                    transfer_id: transfer,
                    restore_id: restore,
                    stage_operation_id: stage,
                },
            ),
        )
        .unwrap();
    let mut stage_request = request(
        &f.source,
        2,
        Action::StageReceiver {
            task_id: f.task,
            transfer_id: transfer,
        },
    );
    stage_request.operation_id = stage;
    f.store.execute(&f.source, &stage_request).unwrap();
    let commit = request(
        &f.source,
        2,
        Action::CommitTransfer {
            task_id: f.task,
            transfer_id: transfer,
            accept_degraded: false,
            lease_ms: 300000,
        },
    );
    let Reply::Transfer(committed) = f.store.execute(&f.source, &commit).unwrap() else {
        panic!()
    };
    assert_eq!(committed.committed_epoch, Some(3));
    assert_eq!(
        f.store.execute(&f.source, &commit).unwrap(),
        Reply::Transfer(committed)
    );
    assert_eq!(
        f.store.execute(&f.receiver, &first_commit).unwrap(),
        first_result
    );
    let (_, mut next_tree, next_hello) =
        launch_fixture_receiver(&mut f.store, &f.source, f.task, transfer, None);
    let next_proof = next_tree.running_proof().unwrap();
    f.store
        .record_receiver_ready(&f.source, transfer, &next_hello, &next_proof, Id::new_v4())
        .unwrap();
    let current = f.store.task(&f.source, f.task).unwrap();
    assert_eq!(current.epoch, 3);
    assert_eq!(current.owner_id, Some(f.source.id()));
    assert_eq!(current.state, TaskState::Active);
    assert_eq!(
        fs::read(f.repo.path().join("file")).unwrap(),
        b"fixture source\n"
    );
    assert_eq!(
        fs::read(f.receiver_spec().working_directory().join("file")).unwrap(),
        b"receiver progress\n"
    );
    assert_eq!(
        fs::read(
            f.temp
                .path()
                .join("state/checkpoints/restores")
                .join(restore.to_string())
                .join("file")
        )
        .unwrap(),
        b"receiver progress\n"
    );
    let stale = request(
        &f.source,
        1,
        Action::Heartbeat {
            task_id: f.task,
            lease_id: f.lease.lease_id,
            lease_ms: 1000,
        },
    );
    assert_eq!(
        f.store.execute(&f.source, &stale).unwrap_err().code,
        ErrorCode::StaleEpoch
    );
}

// M4-SEQ05; FR-08/12/14, INV-02/04/07/09: four hard kills preserve uncertain tool intent without replay.
#[cfg(windows)]
#[test]
fn first_receiver_tool_crashes_never_replay_unknown_writes() {
    for point in [
        "receiver_tool_before_intent",
        "receiver_tool_after_intent",
        "receiver_tool_after_write",
        "receiver_tool_after_completion",
    ] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let task = f.store.task(&f.receiver, f.task).unwrap();
        let target = f.receiver_spec().working_directory().to_path_buf();
        let parent = target.parent().unwrap();
        let stem = f.restore.to_string();
        fs::write(
            parent.join(format!("{stem}.controlled")),
            b"wait for tool decision",
        )
        .unwrap();
        let hello = ReceiverSession {
            schema_version: SchemaVersion::V0_1_0,
            binding: RunBinding {
                run_id: Id::new_v4(),
                task_id: f.task,
                actor_id: f.receiver.id(),
                lease_id: task.lease_id.unwrap(),
                epoch: 2,
            },
            session_id: Id::new_v4(),
            adapter: AgentMetadata {
                name: "fixture".into(),
                version: "1".into(),
            },
        };
        let r = request(
            &f.receiver,
            2,
            Action::AcknowledgeReceiver {
                task_id: f.task,
                transfer_id: f.transfer,
                session: Box::new(hello.clone()),
            },
        );
        let root = f.temp.path().join("state");
        drop(f.store);
        super::tests::kill_request_with_scope(
            &root,
            &f.receiver,
            &r,
            point,
            &f.temp.path().join("ready"),
            Some(f.task),
        );
        // The receiver's exclusive handle must close on coordinator job teardown; no PID adoption.
        let until = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if fs::OpenOptions::new()
                .write(true)
                .open(parent.join(format!("{stem}.alive")))
                .is_ok()
            {
                break;
            }
            assert!(
                std::time::Instant::now() < until,
                "receiver survived coordinator kill"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
        store
            .execute(
                &f.receiver,
                &request(
                    &f.receiver,
                    2,
                    Action::ObserveRun {
                        task_id: f.task,
                        run_id: hello.binding.run_id,
                        lease_id: hello.binding.lease_id,
                        observation: RunObservation::SupervisorLost,
                    },
                ),
            )
            .unwrap();
        let run = store
            .run(&f.receiver, f.task, hello.binding.run_id)
            .unwrap();
        match point {
            "receiver_tool_before_intent" => assert!(run.effects.is_empty()),
            "receiver_tool_after_completion" => {
                assert_eq!(run.effects[0].state, EffectState::Completed)
            }
            _ => assert_eq!(run.effects[0].state, EffectState::Uncertain),
        }
        let tool: Request =
            decode(&fs::read_to_string(parent.join(format!("{stem}.tool-request"))).unwrap())
                .unwrap();
        assert!(store.execute(&f.receiver, &tool).is_err()); // no native proof and no reconstructed permit
        let expected: &[u8] = if matches!(
            point,
            "receiver_tool_after_write" | "receiver_tool_after_completion"
        ) {
            b"receiver progress\n"
        } else {
            b"fixture source\n"
        };
        assert_eq!(fs::read(target.join("file")).unwrap(), expected);
        assert_eq!(
            fs::read(f.repo.path().join("file")).unwrap(),
            b"fixture source\n"
        );
        let current = store.task(&f.receiver, f.task).unwrap();
        assert_eq!(current.state, TaskState::RecoveryRequired);
        assert_eq!(current.epoch, 2);
        assert_eq!(current.owner_id, Some(f.receiver.id()));
        assert_eq!(
            store
                .connection
                .query_row("SELECT count(*) FROM runs", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

pub(super) fn crash_receiver_tool(store: &mut Store, actor: &Actor, r: &Request) {
    let Action::AcknowledgeReceiver {
        task_id,
        transfer_id,
        ref session,
    } = r.action
    else {
        panic!()
    };
    let (run, mut tree, hello) =
        launch_fixture_receiver(store, actor, task_id, transfer_id, Some(session));
    {
        let proof = tree.running_proof().unwrap();
        store
            .record_receiver_ready(actor, transfer_id, &hello, &proof, r.operation_id)
            .unwrap();
    }
    let target = PathBuf::from(
        store
            .transfer(actor, task_id, transfer_id)
            .unwrap()
            .receiver_stage
            .unwrap()
            .report
            .unwrap()
            .worktree,
    );
    fixture_tool(store, actor, &run.binding, &target, &mut tree);
}

// M4-TL01; FR-08/12/14, INV-02/04/07: admission is fresh-only and revoked by quiescing.
#[test]
fn tool_admission_cannot_replay_or_bypass_a_pending_transfer() {
    for freeze in [false, true] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let (run, mut tree, hello) = f.start_receiver();
        let proof = tree.running_proof().unwrap();
        f.store
            .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
            .unwrap();
        let reserve = request(
            &f.receiver,
            2,
            Action::ObserveRun {
                task_id: f.task,
                run_id: run.binding.run_id,
                lease_id: run.binding.lease_id,
                observation: RunObservation::ToolReserved {
                    source_sequence: 1,
                    effect_id: Id::new_v4(),
                    class: EffectClass::LocalReversible,
                },
            },
        );
        let mut raw = reserve.clone();
        raw.operation_id = Id::new_v4();
        assert_eq!(
            f.store.execute(&f.receiver, &raw).unwrap_err().code,
            ErrorCode::ExecutionNotEnabled
        );
        let mut external = reserve.clone();
        external.operation_id = Id::new_v4();
        if let Action::ObserveRun {
            observation: RunObservation::ToolReserved { ref mut class, .. },
            ..
        } = external.action
        {
            *class = EffectClass::ExternalNonIdempotent;
        }
        assert_eq!(
            f.store
                .prepare_tool(&f.receiver, &external, &proof)
                .err()
                .unwrap()
                .code,
            ErrorCode::Blocked
        );
        let (_, permit) = f.store.prepare_tool(&f.receiver, &reserve, &proof).unwrap();
        f.store = Store::open(&f.temp.path().join("state"), Arc::new(FixedClock)).unwrap();
        assert!(f
            .store
            .prepare_tool(&f.receiver, &reserve, &proof)
            .unwrap()
            .1
            .is_none());
        if freeze {
            f.store
                .execute(
                    &f.receiver,
                    &request(
                        &f.receiver,
                        2,
                        Action::RequestTransfer {
                            task_id: f.task,
                            lease_id: run.binding.lease_id,
                            transfer_id: Id::new_v4(),
                            receiver_id: f.source.id(),
                        },
                    ),
                )
                .unwrap();
            assert_eq!(
                f.store
                    .consume_tool(&f.receiver, permit.unwrap(), &proof)
                    .unwrap_err()
                    .code,
                ErrorCode::Blocked
            );
        } else {
            f.store
                .consume_tool(&f.receiver, permit.unwrap(), &proof)
                .unwrap();
        }
    }
}

// M4-TL02; FR-03/14, INV-04/08/09: late facts are retained and force recovery, not discarded as admission requests.
#[test]
fn late_tool_observation_after_quiescing_is_recorded_without_authority() {
    let mut f = Fixture::new(true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    let (run, mut tree, hello) = f.start_receiver();
    let proof = tree.running_proof().unwrap();
    f.store
        .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
        .unwrap();
    let transfer = Id::new_v4();
    f.store
        .execute(
            &f.receiver,
            &request(
                &f.receiver,
                2,
                Action::RequestTransfer {
                    task_id: f.task,
                    lease_id: run.binding.lease_id,
                    transfer_id: transfer,
                    receiver_id: f.source.id(),
                },
            ),
        )
        .unwrap();
    let effect = Id::new_v4();
    f.store
        .execute(
            &f.receiver,
            &request(
                &f.receiver,
                2,
                Action::ObserveRun {
                    task_id: f.task,
                    run_id: run.binding.run_id,
                    lease_id: run.binding.lease_id,
                    observation: RunObservation::ToolStarted {
                        source_sequence: 1,
                        effect_id: effect,
                        class: EffectClass::Unknown,
                    },
                },
            ),
        )
        .unwrap();
    let current = f
        .store
        .run(&f.receiver, f.task, run.binding.run_id)
        .unwrap();
    assert!(current
        .effects
        .iter()
        .any(|e| e.effect_id == effect && e.state == EffectState::Started));
    assert_eq!(
        f.store.task(&f.receiver, f.task).unwrap().state,
        TaskState::RecoveryRequired
    );
    assert_eq!(
        f.store
            .transfer(&f.receiver, f.task, transfer)
            .unwrap()
            .state,
        TransferState::RecoveryRequired
    );
    assert_eq!(f.store.task(&f.receiver, f.task).unwrap().epoch, 2);
}

// M4-SEQ02; INV-02/04/06/09: fixture assumptions cannot hide live scopes, source gaps or effects.
#[cfg(windows)]
#[test]
fn successive_transfer_rejects_live_source_gaps_and_unresolved_effects() {
    for failure in ["live", "gap", "effect"] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let (run, mut tree, hello) = f.start_receiver();
        {
            let proof = tree.running_proof().unwrap();
            f.store
                .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
                .unwrap();
        }
        if failure != "live" {
            let effect = Id::new_v4();
            let sequence = if failure == "gap" { 2 } else { 1 };
            for observation in [
                RunObservation::ToolStarted {
                    source_sequence: sequence,
                    effect_id: effect,
                    class: if failure == "gap" {
                        EffectClass::LocalReversible
                    } else {
                        EffectClass::ExternalNonIdempotent
                    },
                },
                RunObservation::ToolFinished {
                    source_sequence: sequence + 1,
                    effect_id: effect,
                    outcome: ToolOutcome::Succeeded,
                },
            ] {
                f.store
                    .execute(
                        &f.receiver,
                        &request(
                            &f.receiver,
                            2,
                            Action::ObserveRun {
                                task_id: f.task,
                                lease_id: run.binding.lease_id,
                                run_id: run.binding.run_id,
                                observation,
                            },
                        ),
                    )
                    .unwrap();
            }
        }
        let transfer = Id::new_v4();
        f.store
            .execute(
                &f.receiver,
                &request(
                    &f.receiver,
                    2,
                    Action::RequestTransfer {
                        task_id: f.task,
                        lease_id: run.binding.lease_id,
                        transfer_id: transfer,
                        receiver_id: f.source.id(),
                    },
                ),
            )
            .unwrap();
        if failure != "live" {
            tree.terminate().unwrap();
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while tree.status().unwrap() != baton_platform::process::TreeStatus::Empty {
                assert!(std::time::Instant::now() < until);
                std::thread::sleep(Duration::from_millis(10));
            }
            let proof = tree.drain_proof().unwrap();
            f.store
                .record_native_drain(&f.receiver, &run.binding, &proof, Id::new_v4())
                .unwrap();
        }
        let checkpoint = Id::new_v4();
        f.store
            .execute(
                &f.receiver,
                &request(
                    &f.receiver,
                    2,
                    Action::CaptureCheckpoint {
                        task_id: f.task,
                        lease_id: run.binding.lease_id,
                        checkpoint_id: checkpoint,
                        include_untracked: vec![],
                    },
                ),
            )
            .unwrap();
        let Reply::Transfer(blocked) = f
            .store
            .execute(
                &f.receiver,
                &request(
                    &f.receiver,
                    2,
                    Action::PreflightTransfer {
                        task_id: f.task,
                        lease_id: run.binding.lease_id,
                        transfer_id: transfer,
                        checkpoint_id: checkpoint,
                    },
                ),
            )
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            blocked.state,
            TransferState::Blocked,
            "failure {failure} was not blocked"
        );
        assert!(blocked.reasons.contains(&match failure {
            "gap" => TransferReason::EventCoverageIncomplete,
            "effect" => TransferReason::UnresolvedEffects,
            _ => TransferReason::SourceFenceUnavailable,
        }));
        assert_eq!(f.store.task(&f.receiver, f.task).unwrap().epoch, 2);
        assert_eq!(blocked.committed_epoch, None);
    }
}

// M4-SEQ03; FR-12, INV-06/08: timeout, uncertainty and abort while quiescing never revive authority.
#[test]
fn quiescing_expiry_loss_and_abort_preserve_current_owner() {
    for failure in ["expiry", "loss", "abort"] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let (run, mut tree, hello) = f.start_receiver();
        {
            let proof = tree.running_proof().unwrap();
            f.store
                .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
                .unwrap();
        }
        let transfer = Id::new_v4();
        f.store
            .execute(
                &f.receiver,
                &request(
                    &f.receiver,
                    2,
                    Action::RequestTransfer {
                        task_id: f.task,
                        lease_id: run.binding.lease_id,
                        transfer_id: transfer,
                        receiver_id: f.source.id(),
                    },
                ),
            )
            .unwrap();
        match failure {
            "expiry" => {
                f.store
                    .connection
                    .execute("UPDATE clock SET last_ms=301000 WHERE singleton=1", [])
                    .unwrap();
                assert!(f
                    .store
                    .execute(
                        &f.receiver,
                        &request(
                            &f.receiver,
                            2,
                            Action::Heartbeat {
                                task_id: f.task,
                                lease_id: run.binding.lease_id,
                                lease_ms: 10
                            }
                        )
                    )
                    .is_err());
            }
            "loss" => {
                f.store
                    .execute(
                        &f.receiver,
                        &request(
                            &f.receiver,
                            2,
                            Action::ObserveRun {
                                task_id: f.task,
                                run_id: run.binding.run_id,
                                lease_id: run.binding.lease_id,
                                observation: RunObservation::SupervisorLost,
                            },
                        ),
                    )
                    .unwrap();
            }
            _ => {
                f.store
                    .execute(
                        &f.receiver,
                        &request(
                            &f.receiver,
                            2,
                            Action::AbortTransfer {
                                task_id: f.task,
                                transfer_id: transfer,
                            },
                        ),
                    )
                    .unwrap();
            }
        }
        let task = f.store.task(&f.receiver, f.task).unwrap();
        assert_eq!(task.state, TaskState::RecoveryRequired);
        assert_eq!(task.owner_id, Some(f.receiver.id()));
        assert_eq!(task.epoch, 2);
        assert_eq!(
            tree.status().unwrap(),
            baton_platform::process::TreeStatus::Active
        );
        assert!(f
            .store
            .execute(
                &f.source,
                &request(
                    &f.source,
                    2,
                    Action::BeginReceiverStaging {
                        task_id: f.task,
                        transfer_id: transfer,
                        restore_id: Id::new_v4(),
                        stage_operation_id: Id::new_v4()
                    }
                )
            )
            .is_err());
        assert_eq!(f.store.task(&f.receiver, f.task).unwrap().epoch, 2);
    }
}

// M4-SEQ04; T-008: concurrent requests from an active receiver retain one pending handoff.
#[test]
fn quiescing_requests_from_active_receiver_are_serialized_and_idempotent() {
    for duplicate in [false, true] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let (run, mut tree, hello) = f.start_receiver();
        {
            let proof = tree.running_proof().unwrap();
            f.store
                .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
                .unwrap();
        }
        let original = request(
            &f.receiver,
            2,
            Action::RequestTransfer {
                task_id: f.task,
                lease_id: run.binding.lease_id,
                transfer_id: Id::new_v4(),
                receiver_id: f.source.id(),
            },
        );
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|index| {
                let root = f.temp.path().join("state");
                let actor = f.receiver.clone();
                let mut r = original.clone();
                if !duplicate && index == 1 {
                    r.operation_id = Id::new_v4();
                    if let Action::RequestTransfer {
                        ref mut transfer_id,
                        ..
                    } = r.action
                    {
                        *transfer_id = Id::new_v4();
                    }
                }
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
                    barrier.wait();
                    store.execute(&actor, &r)
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            if duplicate { 2 } else { 1 }
        );
        if duplicate {
            assert_eq!(results[0], results[1]);
        } else {
            assert_eq!(
                results
                    .iter()
                    .filter(|r| matches!(r, Err(e) if e.code == ErrorCode::Blocked))
                    .count(),
                1
            );
        }
        assert_eq!(
            f.store
                .connection
                .query_row("SELECT count(*) FROM transfers WHERE active=1", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
        assert_eq!(f.store.task(&f.receiver, f.task).unwrap().epoch, 2);
    }
}

fn fixture_tool(
    store: &mut Store,
    actor: &Actor,
    binding: &RunBinding,
    target: &Path,
    tree: &mut baton_platform::process::ProcessTree,
) -> RunRecord {
    use std::io::Write;
    let parent = target.parent().unwrap();
    let restore = target.file_name().unwrap().to_str().unwrap();
    let effect = Id::new_v4();
    let start = request(
        actor,
        binding.epoch,
        Action::ObserveRun {
            task_id: binding.task_id,
            lease_id: binding.lease_id,
            run_id: binding.run_id,
            observation: RunObservation::ToolReserved {
                source_sequence: 1,
                effect_id: effect,
                class: EffectClass::LocalReversible,
            },
        },
    );
    fs::write(
        parent.join(format!("{restore}.tool-request")),
        encode(&start).unwrap(),
    )
    .unwrap();
    crash_point("receiver_tool_before_intent");
    let proof = tree.running_proof().unwrap();
    let (intent, permit) = store.prepare_tool(actor, &start, &proof).unwrap();
    assert_eq!(intent.effects[0].state, EffectState::Reserved);
    assert!(store
        .prepare_tool(actor, &start, &proof)
        .unwrap()
        .1
        .is_none());
    crash_point("receiver_tool_after_intent");
    store.consume_tool(actor, permit.unwrap(), &proof).unwrap();
    let go_stage = parent.join(format!("{restore}.go-staging"));
    let mut go = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&go_stage)
        .unwrap();
    go.write_all(encode(&start).unwrap().as_bytes()).unwrap();
    go.sync_all().unwrap();
    drop(go);
    fs::hard_link(go_stage, parent.join(format!("{restore}.go"))).unwrap();
    let done = parent.join(format!("{restore}.tool-done"));
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while !done.exists() {
        assert!(!tree.root_exited().unwrap());
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        decode::<Id>(&fs::read_to_string(done).unwrap()).unwrap(),
        start.operation_id
    );
    assert_eq!(
        fs::read(target.join("file")).unwrap(),
        b"receiver progress\n"
    );
    crash_point("receiver_tool_after_write");
    store
        .execute(
            actor,
            &request(
                actor,
                binding.epoch,
                Action::ObserveRun {
                    task_id: binding.task_id,
                    lease_id: binding.lease_id,
                    run_id: binding.run_id,
                    observation: RunObservation::ToolStarted {
                        source_sequence: 2,
                        effect_id: effect,
                        class: EffectClass::LocalReversible,
                    },
                },
            ),
        )
        .unwrap();
    let finish = request(
        actor,
        binding.epoch,
        Action::ObserveRun {
            task_id: binding.task_id,
            lease_id: binding.lease_id,
            run_id: binding.run_id,
            observation: RunObservation::ToolFinished {
                source_sequence: 3,
                effect_id: effect,
                outcome: ToolOutcome::Succeeded,
            },
        },
    );
    fs::write(
        parent.join(format!("{restore}.tool-finish-request")),
        encode(&finish).unwrap(),
    )
    .unwrap();
    let Reply::Run(record) = store.execute(actor, &finish).unwrap() else {
        panic!()
    };
    crash_point("receiver_tool_after_completion");
    record
}

#[test]
#[ignore = "managed fixture child, explicitly invoked by receiver readiness tests"]
fn receiver_fixture_child() {
    use std::io::Write;
    let cwd = std::env::current_dir().unwrap();
    let restore = cwd.file_name().unwrap().to_str().unwrap();
    let parent = cwd.parent().unwrap();
    if parent.join(format!("{restore}.ready")).exists() {
        loop {
            std::thread::park();
        }
    }
    let hello: ReceiverSession =
        decode(&fs::read_to_string(parent.join(format!("{restore}.challenge"))).unwrap()).unwrap();
    #[cfg(windows)]
    let _held = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(parent.join(format!("{restore}.alive")))
            .unwrap()
    };
    let controlled = parent.join(format!("{restore}.controlled")).exists();
    let staged = parent.join(format!("{restore}.ready-staging"));
    if !controlled {
        fs::write(cwd.join("file"), b"receiver progress\n").unwrap();
    }
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .unwrap();
    out.write_all(encode(&hello).unwrap().as_bytes()).unwrap();
    out.sync_all().unwrap();
    drop(out);
    fs::hard_link(staged, parent.join(format!("{restore}.ready"))).unwrap();
    if controlled {
        let go = parent.join(format!("{restore}.go"));
        while !go.exists() {
            std::thread::sleep(Duration::from_millis(10));
        }
        let r: Request = decode(&fs::read_to_string(go).unwrap()).unwrap();
        let Action::ObserveRun {
            task_id,
            run_id,
            lease_id,
            observation:
                RunObservation::ToolReserved {
                    source_sequence: 1,
                    class: EffectClass::LocalReversible,
                    ..
                },
        } = r.action
        else {
            panic!()
        };
        assert_eq!(r.actor_id, hello.binding.actor_id);
        assert_eq!(r.expected_epoch, hello.binding.epoch);
        assert_eq!(task_id, hello.binding.task_id);
        assert_eq!(run_id, hello.binding.run_id);
        assert_eq!(lease_id, hello.binding.lease_id);
        fs::write(cwd.join("file"), b"receiver progress\n").unwrap();
        let done_stage = parent.join(format!("{restore}.tool-done-staging"));
        let mut done = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&done_stage)
            .unwrap();
        done.write_all(encode(&r.operation_id).unwrap().as_bytes())
            .unwrap();
        done.sync_all().unwrap();
        drop(done);
        fs::hard_link(done_stage, parent.join(format!("{restore}.tool-done"))).unwrap();
    }
    loop {
        std::thread::park();
    }
}

// M4-HS02; INV-02/07/09: serialized status and unrelated native scopes cannot acknowledge readiness.
#[test]
fn receiver_readiness_rejects_forged_stale_and_wrong_scope_inputs() {
    let mut f = Fixture::new(true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    let (run, mut tree, hello) = f.start_receiver();
    let r = request(
        &f.receiver,
        2,
        Action::AcknowledgeReceiver {
            task_id: f.task,
            transfer_id: f.transfer,
            session: Box::new(hello.clone()),
        },
    );
    assert_eq!(
        f.store.execute(&f.receiver, &r).unwrap_err().code,
        ErrorCode::ExecutionNotEnabled
    );
    let mut local = r.clone();
    local.operation_id = Id::new_v4();
    assert_eq!(
        f.store.execute_local(&f.receiver, &local).unwrap_err().code,
        ErrorCode::Forbidden
    );
    let proof = tree.running_proof().unwrap();
    let mut stale = hello.clone();
    stale.binding.epoch = 1;
    assert!(f
        .store
        .record_receiver_ready(&f.receiver, f.transfer, &stale, &proof, Id::new_v4())
        .is_err());
    let mut wrong = hello.clone();
    wrong.binding.lease_id = Id::new_v4();
    assert_eq!(
        f.store
            .record_receiver_ready(&f.receiver, f.transfer, &wrong, &proof, Id::new_v4())
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.store
            .record_receiver_ready(&f.source, f.transfer, &hello, &proof, Id::new_v4())
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    // A second actual scope running the same descriptor still has a different identity.
    let cwd = f.receiver_spec().working_directory().to_path_buf();
    let other_spec = baton_platform::process::LaunchSpec::new(
        &std::env::current_exe().unwrap(),
        &cwd,
        [
            "--exact".into(),
            "engine::transfer_tests::receiver_fixture_child".into(),
            "--ignored".into(),
        ],
    )
    .unwrap();
    let mut other = baton_platform::process::ProcessTree::spawn(other_spec).unwrap();
    let other_proof = other.running_proof().unwrap();
    assert_eq!(
        f.store
            .record_receiver_ready(&f.receiver, f.transfer, &hello, &other_proof, Id::new_v4())
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.store
            .run(&f.receiver, f.task, run.binding.run_id)
            .unwrap()
            .state,
        RunState::Running
    );
    assert_eq!(
        f.store.task(&f.receiver, f.task).unwrap().state,
        TaskState::Activating
    );
}

// M4-HS03; INV-01/08: ACTIVE task metadata must agree with the audited readiness grant.
#[test]
fn receiver_readiness_task_without_ack_is_corrupt() {
    let mut f = Fixture::new(true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    let (_run, _tree, _hello) = f.start_receiver();
    f.store
        .connection
        .execute(
            "UPDATE tasks SET record=json_set(record,'$.state','ACTIVE') WHERE id=?1",
            [f.task.to_string()],
        )
        .unwrap();
    assert_eq!(
        f.store.task(&f.receiver, f.task).unwrap_err().code,
        ErrorCode::CorruptStore
    );
}

// M4-HS06; T-003/T-008: actual coordinator kills never reconstruct receiver activation.
#[test]
fn receiver_readiness_crashes_preserve_epoch_and_deny_serialized_replay() {
    for point in [
        "receiver_ready_before_commit",
        "receiver_ready_after_commit",
    ] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let task = f.store.task(&f.receiver, f.task).unwrap();
        let hello = ReceiverSession {
            schema_version: SchemaVersion::V0_1_0,
            binding: RunBinding {
                task_id: f.task,
                run_id: Id::new_v4(),
                epoch: 2,
                actor_id: f.receiver.id(),
                lease_id: task.lease_id.unwrap(),
            },
            session_id: Id::new_v4(),
            adapter: AgentMetadata {
                name: "fixture".into(),
                version: "1".into(),
            },
        };
        let r = request(
            &f.receiver,
            2,
            Action::AcknowledgeReceiver {
                task_id: f.task,
                transfer_id: f.transfer,
                session: Box::new(hello),
            },
        );
        let root = f.temp.path().join("state");
        drop(f.store);
        super::tests::kill_request_with_scope(
            &root,
            &f.receiver,
            &r,
            point,
            &f.temp.path().join("ready"),
            Some(f.task),
        );
        let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
        let task = store.task(&f.receiver, f.task).unwrap();
        assert_eq!(task.epoch, 2);
        assert_eq!(task.owner_id, Some(f.receiver.id()));
        assert_eq!(
            task.state,
            if point == "receiver_ready_after_commit" {
                TaskState::Active
            } else {
                TaskState::Activating
            }
        );
        let count = store
            .events(&f.receiver, f.task)
            .unwrap()
            .iter()
            .filter(|event| event.kind == EventKind::ReceiverReady)
            .count();
        assert_eq!(count, usize::from(point == "receiver_ready_after_commit"));
        assert!(store.execute(&f.receiver, &r).is_err());
        store
            .execute(
                &f.receiver,
                &request(
                    &f.receiver,
                    2,
                    Action::RecoverTransfer {
                        task_id: f.task,
                        transfer_id: f.transfer,
                    },
                ),
            )
            .unwrap();
        assert_eq!(
            store.task(&f.receiver, f.task).unwrap().state,
            TaskState::RecoveryRequired
        );
        assert_eq!(
            store
                .connection
                .query_row("SELECT count(*) FROM runs", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            fs::read(f.repo.path().join("file")).unwrap(),
            b"fixture source\n"
        );
    }
}

pub(super) fn crash_receiver_ready(store: &mut Store, actor: &Actor, r: &Request) {
    let Action::AcknowledgeReceiver {
        task_id,
        transfer_id,
        ref session,
    } = r.action
    else {
        panic!()
    };
    let (_, mut tree, hello) =
        launch_fixture_receiver(store, actor, task_id, transfer_id, Some(session));
    let proof = tree.running_proof().unwrap();
    store
        .record_receiver_ready(actor, transfer_id, &hello, &proof, r.operation_id)
        .unwrap();
}

// M4-HS08; INV-08/09: receiver shutdown preserves native drain evidence and records recovery.
#[cfg(windows)]
#[test]
fn receiver_readiness_native_drain_retains_valid_terminal_evidence() {
    let mut f = Fixture::new(true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    let (run, mut tree, hello) = f.start_receiver();
    {
        let proof = tree.running_proof().unwrap();
        f.store
            .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
            .unwrap();
    }
    tree.terminate().unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while tree.status().unwrap() != baton_platform::process::TreeStatus::Empty {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(tree.running_proof().err().unwrap().code, ErrorCode::Blocked);
    let proof = tree.drain_proof().unwrap();
    f.store
        .record_native_drain(&f.receiver, &run.binding, &proof, Id::new_v4())
        .unwrap();
    f.store
        .native_drain(&f.receiver, f.task, run.binding.run_id)
        .unwrap();
    assert_eq!(
        f.store.task(&f.receiver, f.task).unwrap().state,
        TaskState::RecoveryRequired
    );
}

// M4-HS07; INV-06/08: lost supervision or lease expiry never restores the old owner.
#[test]
fn receiver_readiness_recovery_and_expiry_preserve_owner_and_revoke_launch() {
    for expiry in [false, true] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let (run, mut tree, hello) = f.start_receiver();
        let proof = tree.running_proof().unwrap();
        f.store
            .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
            .unwrap();
        if expiry {
            f.store
                .connection
                .execute("UPDATE clock SET last_ms=301000 WHERE singleton=1", [])
                .unwrap();
        }
        f.store
            .execute(
                &f.receiver,
                &request(
                    &f.receiver,
                    2,
                    Action::ObserveRun {
                        task_id: f.task,
                        run_id: run.binding.run_id,
                        lease_id: run.binding.lease_id,
                        observation: RunObservation::SupervisorLost,
                    },
                ),
            )
            .unwrap();
        let task = f.store.task(&f.receiver, f.task).unwrap();
        assert_eq!(task.state, TaskState::RecoveryRequired);
        assert_eq!(task.owner_id, Some(f.receiver.id()));
        assert_eq!(task.epoch, 2);
        assert!(f
            .store
            .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
            .is_err());
        assert!(f
            .store
            .prepare_receiver_launch(
                &f.receiver,
                &f.activation_request(),
                Id::new_v4(),
                &f.receiver_spec(),
                receiver_capabilities()
            )
            .is_err());
    }
}

// M4-HS04; INV-02/06/09: event gaps and missing engine checks cannot become readiness.
#[test]
fn receiver_readiness_missing_proofs_and_event_gaps_remain_blocked() {
    for gap in [false, true] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let (run, mut tree, hello) = f.start_receiver();
        if gap {
            f.store
                .execute(
                    &f.receiver,
                    &request(
                        &f.receiver,
                        2,
                        Action::ObserveRun {
                            task_id: f.task,
                            lease_id: run.binding.lease_id,
                            run_id: run.binding.run_id,
                            observation: RunObservation::ToolStarted {
                                source_sequence: 2,
                                effect_id: Id::new_v4(),
                                class: EffectClass::Unknown,
                            },
                        },
                    ),
                )
                .unwrap();
        } else {
            transfers::simulate_boundary(Id::nil());
        }
        let proof = tree.running_proof().unwrap();
        assert_eq!(
            f.store
                .record_receiver_ready(&f.receiver, f.transfer, &hello, &proof, Id::new_v4())
                .unwrap_err()
                .code,
            ErrorCode::Blocked
        );
        assert_eq!(f.store.task(&f.receiver, f.task).unwrap().epoch, 2);
        assert_eq!(
            f.store.task(&f.receiver, f.task).unwrap().state,
            TaskState::Activating
        );
    }
}

// M4-AC07; T-003/T-008: competing requests serialize to one native launch permit.
#[test]
fn receiver_launch_concurrency_issues_exactly_one_permit() {
    for exact_retry in [false, true] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let r = f.activation_request();
        let run = Id::new_v4();
        let target = f.receiver_spec().working_directory().to_path_buf();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|index| {
                let root = f.temp.path().join("state");
                let actor = f.receiver.clone();
                let mut r = r.clone();
                if !exact_retry && index == 1 {
                    r.operation_id = Id::new_v4();
                }
                let target = target.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    transfers::simulate_boundary(r.action.task_id().unwrap());
                    let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
                    let spec = baton_platform::process::LaunchSpec::new(
                        &std::env::current_exe().unwrap(),
                        &target,
                        ["--help".into()],
                    )
                    .unwrap();
                    barrier.wait();
                    store
                        .prepare_receiver_launch(&actor, &r, run, &spec, receiver_capabilities())
                        .map(|(_, permit)| permit.is_some())
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| matches!(r, Ok(true))).count(), 1);
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            if exact_retry { 2 } else { 1 }
        );
        assert_eq!(f.store.task(&f.receiver, f.task).unwrap().epoch, 2);
        assert_eq!(
            f.store
                .connection
                .query_row("SELECT count(*) FROM runs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

// M4-AC06; FR-08/12, INV-02/07: actual kills cannot reconstruct or duplicate launch permission.
#[test]
fn receiver_launch_crashes_never_reissue_committed_permission() {
    for point in [
        "before_commit",
        "after_commit",
        "native_before_commit",
        "native_after_commit",
    ] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let r = f.activation_request();
        let spec = f.receiver_spec();
        let root = f.temp.path().join("state");
        drop(f.store);
        super::tests::kill_request_with_scope(
            &root,
            &f.receiver,
            &r,
            point,
            &f.temp.path().join("ready"),
            Some(f.task),
        );
        let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
        let task = store.task(&f.receiver, f.task).unwrap();
        assert_eq!(task.epoch, 2);
        assert_eq!(task.owner_id, Some(f.receiver.id()));
        let scope_count: i64 = store
            .connection
            .query_row("SELECT count(*) FROM native_scopes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            scope_count,
            if point == "native_after_commit" { 1 } else { 0 }
        );
        let (_, permit) = store
            .prepare_receiver_launch(
                &f.receiver,
                &r,
                r.operation_id,
                &spec,
                receiver_capabilities(),
            )
            .unwrap();
        assert_eq!(permit.is_some(), point == "before_commit");
        assert!(store
            .prepare_receiver_launch(
                &f.receiver,
                &r,
                r.operation_id,
                &spec,
                receiver_capabilities()
            )
            .unwrap()
            .1
            .is_none());
        assert_eq!(
            store
                .connection
                .query_row("SELECT count(*) FROM runs", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .events(&f.receiver, f.task)
                .unwrap()
                .iter()
                .filter(|e| e.kind == EventKind::ReceiverActivationReserved)
                .count(),
            1
        );
        store
            .execute(
                &f.receiver,
                &request(
                    &f.receiver,
                    2,
                    Action::RecoverTransfer {
                        task_id: f.task,
                        transfer_id: f.transfer,
                    },
                ),
            )
            .unwrap();
        assert_eq!(
            store.task(&f.receiver, f.task).unwrap().state,
            TaskState::RecoveryRequired
        );
    }
}

// M4-AC01; FR-08/12/13, INV-02/07/08: a native launch reservation is fresh-only, not vendor ACTIVE.
#[test]
fn receiver_launch_permit_is_fresh_only_and_binds_native_scope() {
    let mut f = Fixture::new_with_run(true, true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    let r = f.activation_request();
    let spec = f.receiver_spec();
    let run_id = Id::new_v4();
    let (run, permit) = f
        .store
        .prepare_receiver_launch(&f.receiver, &r, run_id, &spec, receiver_capabilities())
        .unwrap();
    let permit = permit.unwrap();
    assert_eq!(run.binding.epoch, 2);
    assert_eq!(
        f.store
            .connection
            .query_row("SELECT count(*) FROM runs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        f.store.task(&f.receiver, f.task).unwrap().state,
        TaskState::Activating
    );
    assert!(f
        .store
        .prepare_receiver_launch(&f.receiver, &r, run_id, &spec, receiver_capabilities())
        .unwrap()
        .1
        .is_none());
    f.store.validate_launch(&f.receiver, &permit).unwrap();
    let tree = baton_platform::process::ProcessTree::spawn(spec).unwrap();
    f.store
        .record_native_spawn(&f.receiver, &permit, &tree, Id::new_v4())
        .unwrap();
    assert!(f.store.validate_launch(&f.receiver, &permit).is_err());
    let transfer = f.store.transfer(&f.receiver, f.task, f.transfer).unwrap();
    assert_eq!(transfer.state, TransferState::Activating);
    assert_eq!(transfer.quality, RecoveryQuality::Blocked);
    assert_eq!(transfer.receiver_activation.unwrap().binding, run.binding);
    f.store = Store::open(&f.temp.path().join("state"), Arc::new(FixedClock)).unwrap();
    assert!(f
        .store
        .prepare_receiver_launch(
            &f.receiver,
            &r,
            run_id,
            &f.receiver_spec(),
            receiver_capabilities()
        )
        .unwrap()
        .1
        .is_none());
    assert!(f
        .store
        .prepare_receiver_launch(
            &f.receiver,
            &f.activation_request(),
            Id::new_v4(),
            &f.receiver_spec(),
            receiver_capabilities()
        )
        .is_err());
    f.store
        .execute(
            &f.receiver,
            &request(
                &f.receiver,
                2,
                Action::RecoverTransfer {
                    task_id: f.task,
                    transfer_id: f.transfer,
                },
            ),
        )
        .unwrap();
    let task = f.store.task(&f.receiver, f.task).unwrap();
    assert_eq!(task.state, TaskState::RecoveryRequired);
    assert_eq!(task.epoch, 2);
    assert_eq!(task.owner_id, Some(f.receiver.id()));
}

// M4-AC03; INV-02/03/05/07: precommit, wrong workspace, drift and conflicting retries refuse.
#[test]
fn receiver_launch_rejects_early_wrong_workspace_and_receiver_edits() {
    let mut f = Fixture::new(true);
    f.ready();
    let spec = f.receiver_spec();
    let mut early = f.activation_request();
    early.expected_epoch = 1;
    assert!(f
        .store
        .prepare_receiver_launch(
            &f.receiver,
            &early,
            Id::new_v4(),
            &spec,
            receiver_capabilities()
        )
        .is_err());
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    let wrong = baton_platform::process::LaunchSpec::new(
        &std::env::current_exe().unwrap(),
        &f.repo.path(),
        ["--help".into()],
    )
    .unwrap();
    assert_eq!(
        f.store
            .prepare_receiver_launch(
                &f.receiver,
                &f.activation_request(),
                Id::new_v4(),
                &wrong,
                receiver_capabilities()
            )
            .err()
            .unwrap()
            .code,
        ErrorCode::Forbidden
    );
    let r = f.activation_request();
    let run = Id::new_v4();
    let (_, permit) = f
        .store
        .prepare_receiver_launch(&f.receiver, &r, run, &spec, receiver_capabilities())
        .unwrap();
    assert_eq!(
        f.store
            .prepare_receiver_launch(
                &f.receiver,
                &r,
                Id::new_v4(),
                &spec,
                receiver_capabilities()
            )
            .err()
            .unwrap()
            .code,
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(
        f.store.execute_local(&f.receiver, &r).unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    fs::write(spec.working_directory().join("file"), b"user edit").unwrap();
    assert!(f
        .store
        .validate_launch(&f.receiver, &permit.unwrap())
        .is_err());
    assert_eq!(
        fs::read(spec.working_directory().join("file")).unwrap(),
        b"user edit"
    );
}

// M4-AC04; INV-01/07: corrupted run metadata cannot back an activation intent.
#[test]
fn receiver_launch_run_corruption_is_detected_on_task_read() {
    let mut f = Fixture::new(true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    let run = Id::new_v4();
    f.store
        .prepare_receiver_launch(
            &f.receiver,
            &f.activation_request(),
            run,
            &f.receiver_spec(),
            receiver_capabilities(),
        )
        .unwrap();
    f.store
        .connection
        .execute(
            "UPDATE runs SET record=json_set(record,'$.launch_fingerprint',?1) WHERE id=?2",
            params!["b".repeat(64), run.to_string()],
        )
        .unwrap();
    assert_eq!(
        f.store.task(&f.receiver, f.task).unwrap_err().code,
        ErrorCode::CorruptStore
    );
}

struct AdvancingClock(std::sync::atomic::AtomicUsize);
impl Clock for AdvancingClock {
    fn now_ms(&self) -> Result<i64, Diagnostic> {
        Ok(
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                1000
            } else {
                301000
            },
        )
    }
}
// M4-AC05; INV-02/06: filesystem validation cannot outlive the receiver lease.
#[test]
fn receiver_launch_expiry_during_validation_refuses_without_owner_rollback() {
    let mut f = Fixture::new(true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    let (_, permit) = f
        .store
        .prepare_receiver_launch(
            &f.receiver,
            &f.activation_request(),
            Id::new_v4(),
            &f.receiver_spec(),
            receiver_capabilities(),
        )
        .unwrap();
    f.store.clock = Arc::new(AdvancingClock(std::sync::atomic::AtomicUsize::new(0)));
    assert_eq!(
        f.store
            .validate_launch(&f.receiver, &permit.unwrap())
            .unwrap_err()
            .code,
        ErrorCode::Blocked
    );
    let task = f.store.task(&f.receiver, f.task).unwrap();
    assert_eq!(task.epoch, 2);
    assert_eq!(task.owner_id, Some(f.receiver.id()));
}

// M4-AC02; T-003/T-009: lost production proofs never issue a receiver permit.
#[test]
fn receiver_launch_rechecks_boundary_and_preserves_committed_owner() {
    let mut f = Fixture::new(true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    transfers::simulate_boundary(Id::nil());
    assert_eq!(
        f.store
            .prepare_receiver_launch(
                &f.receiver,
                &f.activation_request(),
                Id::new_v4(),
                &f.receiver_spec(),
                receiver_capabilities()
            )
            .err()
            .unwrap()
            .code,
        ErrorCode::Blocked
    );
    assert_eq!(f.store.task(&f.receiver, f.task).unwrap().epoch, 2);
    assert_eq!(
        f.store
            .connection
            .query_row("SELECT count(*) FROM runs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

// M4-AC08; FR-12/13, INV-06/08: uncertain receiver launch is explicit recovery, never rollback.
#[test]
fn receiver_launch_loss_and_expiry_record_recovery_without_replay() {
    for expiry in [false, true] {
        let mut f = Fixture::new(true);
        f.ready();
        f.store.execute(&f.receiver, &f.commit_request()).unwrap();
        let r = f.activation_request();
        if expiry {
            f.store.clock = Arc::new(AdvancingClock(std::sync::atomic::AtomicUsize::new(0)));
            assert_eq!(
                f.store
                    .prepare_receiver_launch(
                        &f.receiver,
                        &r,
                        Id::new_v4(),
                        &f.receiver_spec(),
                        receiver_capabilities()
                    )
                    .err()
                    .unwrap()
                    .code,
                ErrorCode::Blocked
            );
        } else {
            let (run, permit) = f
                .store
                .prepare_receiver_launch(
                    &f.receiver,
                    &r,
                    Id::new_v4(),
                    &f.receiver_spec(),
                    receiver_capabilities(),
                )
                .unwrap();
            f.store
                .execute(
                    &f.receiver,
                    &request(
                        &f.receiver,
                        2,
                        Action::ObserveRun {
                            task_id: f.task,
                            run_id: run.binding.run_id,
                            lease_id: run.binding.lease_id,
                            observation: RunObservation::SupervisorLost,
                        },
                    ),
                )
                .unwrap();
            assert_eq!(
                f.store
                    .transfer(&f.receiver, f.task, f.transfer)
                    .unwrap()
                    .state,
                TransferState::RecoveryRequired
            );
            assert!(f
                .store
                .validate_launch(&f.receiver, &permit.unwrap())
                .is_err());
        }
        let task = f.store.task(&f.receiver, f.task).unwrap();
        assert_eq!(task.state, TaskState::RecoveryRequired);
        assert_eq!(task.epoch, 2);
        assert_eq!(task.owner_id, Some(f.receiver.id()));
    }
}

// M4-TX01; T-003/T-008/T-009: real storage/restore and epoch CAS under fixture-only boundary assumptions.
#[test]
fn receiver_stage_commit_retry_and_recovery_preserve_new_epoch() {
    let mut f = Fixture::new(true);
    f.ready();
    let task_id = f.task;
    let open = |actor: &Actor, epoch, lease_id| {
        request(
            actor,
            epoch,
            Action::OpenRun {
                task_id,
                lease_id,
                run_id: Id::new_v4(),
                launch_fingerprint: "a".repeat(64),
                capabilities: AdapterCapabilities {
                    lifecycle: true,
                    tool_outcomes: true,
                    interrupt: true,
                    mutation_gating: true,
                },
            },
        )
    };
    assert_eq!(
        f.store
            .prepare_launch(&f.receiver, &open(&f.receiver, 1, f.lease.lease_id))
            .err()
            .unwrap()
            .code,
        ErrorCode::Forbidden
    );
    let commit = f.commit_request();
    let Reply::Transfer(committed) = f.store.execute(&f.receiver, &commit).unwrap() else {
        panic!()
    };
    assert_eq!(committed.state, TransferState::Committed);
    assert_eq!(committed.committed_epoch, Some(2));
    assert_eq!(committed.quality, RecoveryQuality::Blocked);
    assert_eq!(
        f.store.execute(&f.receiver, &commit).unwrap(),
        Reply::Transfer(committed)
    );
    let task = f.store.task(&f.receiver, f.task).unwrap();
    assert_eq!(task.epoch, 2);
    assert_eq!(task.owner_id, Some(f.receiver.id()));
    assert_eq!(task.state, TaskState::Committed);
    assert_eq!(
        f.store
            .prepare_launch(&f.receiver, &open(&f.receiver, 2, task.lease_id.unwrap()))
            .err()
            .unwrap()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        f.store
            .execute(
                &f.source,
                &request(
                    &f.source,
                    1,
                    Action::Heartbeat {
                        task_id: f.task,
                        lease_id: f.lease.lease_id,
                        lease_ms: 10
                    }
                )
            )
            .unwrap_err()
            .code,
        ErrorCode::StaleEpoch
    );
    assert_eq!(
        f.store
            .execute(
                &f.receiver,
                &request(
                    &f.receiver,
                    2,
                    Action::ActivateReceiver {
                        task_id: f.task,
                        transfer_id: f.transfer
                    }
                )
            )
            .unwrap_err()
            .code,
        ErrorCode::ExecutionNotEnabled
    );
    f.store
        .execute(
            &f.receiver,
            &request(
                &f.receiver,
                2,
                Action::RecoverTransfer {
                    task_id: f.task,
                    transfer_id: f.transfer,
                },
            ),
        )
        .unwrap();
    let task = f.store.task(&f.receiver, f.task).unwrap();
    assert_eq!(task.epoch, 2);
    assert_eq!(task.owner_id, Some(f.receiver.id()));
    assert_eq!(task.state, TaskState::RecoveryRequired);
    assert_eq!(
        fs::read(f.repo.path().join("file")).unwrap(),
        b"fixture source\n"
    );
}

// M4-TX02; production checker stays closed even in the lib-test binary without fixture scope.
#[test]
fn production_missing_boundary_cannot_stage_receiver() {
    let mut f = Fixture::new(false);
    let r = request(
        &f.receiver,
        1,
        Action::BeginReceiverStaging {
            task_id: f.task,
            transfer_id: f.transfer,
            restore_id: f.restore,
            stage_operation_id: f.stage,
        },
    );
    assert_eq!(
        f.store.execute(&f.receiver, &r).unwrap_err().code,
        ErrorCode::Blocked
    );
    assert!(!f
        .temp
        .path()
        .join("state/checkpoints/restores")
        .join(f.restore.to_string())
        .exists());
    assert_eq!(f.store.task(&f.source, f.task).unwrap().epoch, 1);
}

// M4-TX03; T-005/T-008: a staged destination and its request ID cannot be stolen.
#[test]
fn receiver_stage_reserves_request_and_destination_identity() {
    let mut f = Fixture::new(true);
    f.begin();
    let mut stolen = request(
        &f.source,
        1,
        Action::RecordIntent {
            task_id: f.task,
            lease_id: f.lease.lease_id,
            summary: "conflict".into(),
        },
    );
    stolen.operation_id = f.stage;
    assert_eq!(
        f.store.execute(&f.source, &stolen).unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    let checkpoint_id = f
        .store
        .transfer(&f.source, f.task, f.transfer)
        .unwrap()
        .checkpoint
        .unwrap()
        .checkpoint_id;
    stolen.action = Action::RestoreCheckpoint {
        task_id: f.task,
        lease_id: f.lease.lease_id,
        checkpoint_id,
        restore_id: Id::new_v4(),
    };
    assert_eq!(
        f.store.execute(&f.source, &stolen).unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    let count: i64 = f
        .store
        .connection
        .query_row("SELECT count(*) FROM restore_intents", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let stage_request = f.stage_request();
    let first = f.store.execute(&f.receiver, &stage_request).unwrap();
    assert_eq!(f.store.execute(&f.receiver, &stage_request).unwrap(), first);
    assert_eq!(
        f.store.task(&f.source, f.task).unwrap().owner_id,
        Some(f.source.id())
    );
}

// M4-TX04; FR-09, INV-03/05: neither source drift nor receiver edits can commit.
#[test]
fn changed_source_and_receiver_block_commit_without_overwrite() {
    let mut f = Fixture::new(true);
    f.ready();
    fs::write(f.repo.path().join("file"), b"source changed").unwrap();
    let commit = f.commit_request();
    assert_eq!(
        f.store.execute(&f.receiver, &commit).unwrap_err().code,
        ErrorCode::SourceChanged
    );
    assert_eq!(f.store.task(&f.source, f.task).unwrap().epoch, 1);
    fs::write(f.repo.path().join("file"), b"fixture source\n").unwrap();
    let receiver = f
        .temp
        .path()
        .join("state/checkpoints/restores")
        .join(f.restore.to_string())
        .join("file");
    fs::write(&receiver, b"receiver edit").unwrap();
    assert_eq!(
        f.store
            .execute(&f.receiver, &f.commit_request())
            .unwrap_err()
            .code,
        ErrorCode::IntegrityFailure
    );
    assert_eq!(fs::read(receiver).unwrap(), b"receiver edit");
    assert_eq!(
        f.store.task(&f.source, f.task).unwrap().owner_id,
        Some(f.source.id())
    );
}

// M4-TX05; INV-02/08: persisted fixture preparation cannot bypass real checks.
#[test]
fn losing_boundary_evidence_blocks_a_previously_ready_transfer() {
    let mut f = Fixture::new(true);
    f.ready();
    transfers::simulate_boundary(Id::nil());
    let Reply::Transfer(blocked) = f.store.execute(&f.receiver, &f.commit_request()).unwrap()
    else {
        panic!()
    };
    assert_eq!(blocked.state, TransferState::Blocked);
    assert_eq!(blocked.committed_epoch, None);
    assert!(blocked
        .reasons
        .contains(&TransferReason::SourceFenceUnavailable));
    assert_eq!(f.store.task(&f.source, f.task).unwrap().epoch, 1);
}

// M4-TX06; T-001/T-008: concurrent receiver commits allocate only one epoch.
#[test]
fn concurrent_transfer_commits_have_one_owner_change() {
    let mut f = Fixture::new(true);
    f.ready();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let mut store =
                Store::open(&f.temp.path().join("state"), Arc::new(FixedClock)).unwrap();
            let actor = f.receiver.clone();
            let r = f.commit_request();
            let task = f.task;
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                transfers::simulate_boundary(task);
                barrier.wait();
                store.execute(&actor, &r)
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(e) if e.code == ErrorCode::StaleEpoch))
            .count(),
        1
    );
    let task = f.store.task(&f.receiver, f.task).unwrap();
    assert_eq!(task.epoch, 2);
    assert_eq!(task.owner_id, Some(f.receiver.id()));
}

// M4-TX07; T-004/T-008: actual process kills before/after each durable transfer phase.
#[test]
fn transfer_stage_and_commit_crashes_preserve_one_destination_and_epoch() {
    for phase in ["begin", "stage", "commit"] {
        for point in ["before_commit", "after_commit"] {
            let mut f = Fixture::new(true);
            if phase != "begin" {
                f.begin();
            }
            if phase == "commit" {
                f.store.execute(&f.receiver, &f.stage_request()).unwrap();
            }
            let r = match phase {
                "begin" => request(
                    &f.receiver,
                    1,
                    Action::BeginReceiverStaging {
                        task_id: f.task,
                        transfer_id: f.transfer,
                        restore_id: f.restore,
                        stage_operation_id: f.stage,
                    },
                ),
                "stage" => f.stage_request(),
                _ => f.commit_request(),
            };
            let root = f.temp.path().join("state");
            drop(f.store);
            super::tests::kill_request_with_scope(
                &root,
                &f.receiver,
                &r,
                point,
                &f.temp.path().join("ready"),
                Some(f.task),
            );
            let mut store = Store::open(&root, Arc::new(FixedClock)).unwrap();
            let task = store.task(&f.receiver, f.task).unwrap();
            assert_eq!(
                task.epoch,
                if phase == "commit" && point == "after_commit" {
                    2
                } else {
                    1
                }
            );
            let result = store.execute(&f.receiver, &r).unwrap();
            assert_eq!(store.execute(&f.receiver, &r).unwrap(), result);
            assert_eq!(
                store.task(&f.receiver, f.task).unwrap().epoch,
                if phase == "commit" { 2 } else { 1 }
            );
            if phase != "begin" {
                assert_eq!(
                    fs::read(
                        root.join("checkpoints/restores")
                            .join(f.restore.to_string())
                            .join("file")
                    )
                    .unwrap(),
                    b"fixture source\n"
                );
                assert_eq!(
                    fs::read_dir(root.join("checkpoints/restores"))
                        .unwrap()
                        .count(),
                    1
                );
            }
            assert_eq!(
                fs::read(f.repo.path().join("file")).unwrap(),
                b"fixture source\n"
            );
        }
    }
}

// M4-TX08; INV-06/07: committed lease expiry enters recovery without rollback.
#[test]
fn committed_expiry_and_old_source_abort_preserve_receiver_ownership() {
    let mut f = Fixture::new(true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    f.store
        .connection
        .execute("UPDATE clock SET last_ms=301000 WHERE singleton=1", [])
        .unwrap();
    let recover = request(
        &f.receiver,
        2,
        Action::RecoverTransfer {
            task_id: f.task,
            transfer_id: f.transfer,
        },
    );
    f.store.execute(&f.receiver, &recover).unwrap();
    let task = f.store.task(&f.receiver, f.task).unwrap();
    assert_eq!(task.epoch, 2);
    assert_eq!(task.owner_id, Some(f.receiver.id()));
    assert_eq!(task.state, TaskState::RecoveryRequired);
    let abort = request(
        &f.source,
        1,
        Action::AbortTransfer {
            task_id: f.task,
            transfer_id: f.transfer,
        },
    );
    assert_eq!(
        f.store.execute(&f.source, &abort).unwrap_err().code,
        ErrorCode::StaleEpoch
    );
    assert_eq!(
        f.store.execute(&f.receiver, &recover).unwrap(),
        Reply::Transfer(f.store.transfer(&f.receiver, f.task, f.transfer).unwrap())
    );
}

// M4-TX09; INV-01/07: mutable task ownership must agree with the committed grant.
#[test]
fn mismatched_committed_task_owner_is_corrupt() {
    let mut f = Fixture::new(true);
    f.ready();
    f.store.execute(&f.receiver, &f.commit_request()).unwrap();
    f.store
        .connection
        .execute(
            "UPDATE tasks SET record=json_set(record,'$.owner_id',?1) WHERE id=?2",
            params![f.source.id().to_string(), f.task.to_string()],
        )
        .unwrap();
    assert_eq!(
        f.store.task(&f.receiver, f.task).unwrap_err().code,
        ErrorCode::CorruptStore
    );
}
