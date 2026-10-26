use baton_adapter_codex::Source;
use baton_protocol::*;
use baton_store::{Actor, Store, SystemClock};
use baton_supervisor::{LaunchSpec, Session};
use baton_test_support::GitFixture;
use std::{path::Path, sync::Arc, time::Duration};

#[path = "support/admission_probe.rs"]
mod admission_probe;

struct Fixture {
    store: Store,
    actor: Actor,
    repo: GitFixture,
    temp: tempfile::TempDir,
    task: Id,
    lease: Lease,
}
impl Fixture {
    fn new() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }
    fn with_clock(clock: Arc<dyn baton_store::Clock>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::create(&temp.path().join("state"), clock).unwrap();
        let actor = store.create_local_actor().unwrap();
        let repo = GitFixture::new().unwrap();
        let repository_id = Id::new_v4();
        let task = Id::new_v4();
        let req = |action| Request {
            schema_version: SchemaVersion::V0_1_0,
            operation_id: Id::new_v4(),
            actor_id: actor.id(),
            expected_epoch: 0,
            action,
        };
        store
            .execute(
                &actor,
                &req(Action::EnrollRepository {
                    repository_id,
                    root: repo.path().to_str().unwrap().into(),
                }),
            )
            .unwrap();
        store
            .execute(
                &actor,
                &req(Action::CreateTask {
                    repository_id,
                    task_id: task,
                    goal: "synthetic protocol test".into(),
                    scope: ".".into(),
                    agent: AgentMetadata {
                        name: "codex-protocol-fixture".into(),
                        version: "0.1.0".into(),
                    },
                }),
            )
            .unwrap();
        let Reply::Lease(lease) = store
            .execute(
                &actor,
                &req(Action::ClaimTask {
                    task_id: task,
                    lease_ms: 300_000,
                }),
            )
            .unwrap()
        else {
            panic!()
        };
        Self {
            store,
            actor,
            repo,
            temp,
            task,
            lease,
        }
    }
    fn launch_spec(&mut self, spec: LaunchSpec) -> Session {
        let request = Request {
            schema_version: SchemaVersion::V0_1_0,
            operation_id: Id::new_v4(),
            actor_id: self.actor.id(),
            expected_epoch: 1,
            action: Action::OpenRun {
                task_id: self.task,
                lease_id: self.lease.lease_id,
                run_id: Id::new_v4(),
                launch_fingerprint: spec.fingerprint(),
                capabilities: AdapterCapabilities {
                    lifecycle: false,
                    tool_outcomes: false,
                    interrupt: false,
                    mutation_gating: false,
                },
            },
        };
        let (_, permit) = self.store.prepare_launch(&self.actor, &request).unwrap();
        Session::launch(&mut self.store, &self.actor, permit.unwrap(), spec).unwrap()
    }
    fn launch(&mut self, mode: &str) -> Session {
        let spec = LaunchSpec::new(
            Path::new(env!("CARGO_BIN_EXE_baton-codex-protocol-fixture")),
            &self.repo.path(),
            [mode.into(), self.temp.path().as_os_str().to_owned()],
        )
        .unwrap()
        .with_piped_stdio();
        self.launch_spec(spec)
    }
    #[cfg(windows)]
    async fn assert_child_stopped(&self) {
        use std::os::windows::fs::OpenOptionsExt;
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(self.temp.path().join("alive"))
                .is_ok()
            {
                return;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "managed child still holds exclusive handle"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

// M5-C02/C03; FR-03/15, INV-06/09: actual bound pipes, no task activation from handshake.
#[tokio::test]
async fn native_handshake_preserves_task_binding_and_stops_without_starting_a_turn() {
    let mut f = Fixture::new();
    let session = f.launch("good");
    let binding = session.binding().clone();
    let source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(source.binding(), &binding);
    assert_eq!(source.runtime().platform_os, "windows");
    assert_eq!(
        f.store.task(&f.actor, f.task).unwrap().state,
        TaskState::Reserved
    );
    assert!(f
        .store
        .run(&f.actor, f.task, binding.run_id)
        .unwrap()
        .blockers()
        .contains(&BoundaryReason::AdapterUnsupported));
    assert!(source
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .is_ok());
    #[cfg(windows)]
    f.assert_child_stopped().await;
    assert!(!f.repo.path().join("modified").exists());
}

// M5-C02; FR-12/15, INV-04/06/09: failed handshakes retain recovery, never auto-relaunch.
#[tokio::test]
async fn native_invalid_reply_eof_and_timeout_stop_scope_and_record_loss() {
    for (mode, expected) in [
        ("wrong-id", ErrorCode::AdapterProtocol),
        ("oversized", ErrorCode::AdapterProtocol),
        ("partial", ErrorCode::AdapterProtocol),
        ("eof", ErrorCode::AdapterProtocol),
        ("hang", ErrorCode::AdapterTimeout),
    ] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let binding = session.binding().clone();
        let error = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(2))
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, expected, "{mode}");
        let run = f.store.run(&f.actor, f.task, binding.run_id).unwrap();
        assert_eq!(run.state, RunState::RecoveryRequired);
        assert_eq!(f.store.task(&f.actor, f.task).unwrap().epoch, binding.epoch);
        assert!(!f.temp.path().join("acknowledged").exists());
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-C04; INV-04/06/09: cancelling the host future must not abandon a live protocol peer.
#[tokio::test]
async fn cancelled_connect_terminates_scope_and_records_recovery() {
    let mut f = Fixture::new();
    let session = f.launch("hang");
    let run_id = session.binding().run_id;
    let received = f.temp.path().join("received");
    let mut connecting = Box::pin(Source::connect(
        session,
        &mut f.store,
        &f.actor,
        Duration::from_secs(10),
    ));
    tokio::select! {
        _ = &mut connecting => panic!("fixture should wait for cancellation"),
        ready = tokio::time::timeout(Duration::from_secs(5), async {
            while !received.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }) => ready.unwrap(),
    }
    drop(connecting);
    assert_eq!(
        f.store.run(&f.actor, f.task, run_id).unwrap().state,
        RunState::RecoveryRequired
    );
    #[cfg(windows)]
    f.assert_child_stopped().await;
}

fn adapter() -> AgentMetadata {
    AgentMetadata {
        name: "codex".into(),
        version: "0.164.0".into(),
    }
}

// M5-R03; FR-03/06/15, INV-04/07/09: registration is audited, not a model turn.
#[tokio::test]
async fn ephemeral_thread_registration_is_bound_and_cannot_be_replayed() {
    for mode in ["register-good", "register-before", "register-after"] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let run = session.binding().run_id;
        let source = Source::connect_with_file_tools(
            session,
            &mut f.store,
            &f.actor,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let operation = Id::new_v4();
        let (mut source, mut record) = source
            .start_file_thread(
                &mut f.store,
                &f.actor,
                operation,
                "baton-fixture",
                "baton_mock",
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        if mode == "register-after" {
            let captured = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-registered",
                    Duration::from_secs(5),
                )
                .await
                .unwrap();
            source = captured.0;
            record = captured.1;
        }
        if mode != "register-good" {
            assert_eq!(
                record.announced_thread.as_deref(),
                Some("thread-registered")
            );
        }
        assert_eq!(record.source_thread.as_deref(), Some("thread-registered"));
        assert_eq!(record.effects.len(), 3);
        assert_eq!(record.effects[0].state, EffectState::Completed);
        assert_eq!(record.effects[1].state, EffectState::Completed);
        assert_eq!(record.effects[2].class, EffectClass::Unknown);
        assert_eq!(record.effects[2].state, EffectState::Uncertain);
        assert!(!record.event_gap);
        let events = f.store.execution_events(&f.actor, f.task, run).unwrap();
        let json = serde_json::to_string(&events).unwrap();
        assert!(
            !json.contains("private preview")
                && !json.contains("private-id")
                && !json.contains("private-name")
        );
        assert!(events.iter().any(|event| matches!(&event.observation, RunObservation::ThreadRegistered { thread_id, .. } if thread_id=="thread-registered") && event.origin==ExecutionOrigin::AdapterObserved));
        let error = source
            .start_file_thread(
                &mut f.store,
                &f.actor,
                operation,
                "baton-fixture",
                "baton_mock",
                Duration::from_secs(5),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::Blocked);
        assert!(!f.temp.path().join("registration-replayed").exists());
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-SP02; reported executable services must be refused before thread/start.
#[tokio::test]
async fn startup_configuration_is_checked_before_thread_registration() {
    for mode in [
        "register-config-mcp",
        "register-config-hooks",
        "register-config-plugins",
        "register-config-marketplaces",
        "register-config-notify",
    ] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let run = session.binding().run_id;
        let source = Source::connect_with_file_tools(
            session,
            &mut f.store,
            &f.actor,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let result = source
            .start_file_thread(
                &mut f.store,
                &f.actor,
                Id::new_v4(),
                "baton-fixture",
                "baton_mock",
                Duration::from_secs(5),
            )
            .await;
        assert!(
            matches!(result, Err(ref error) if error.code == ErrorCode::ExecutionNotEnabled),
            "unsafe startup config accepted: {mode}"
        );
        assert!(f.temp.path().join("config-received").exists());
        assert!(!f.temp.path().join("registration-received").exists());
        let record = f.store.run(&f.actor, f.task, run).unwrap();
        assert!(record.source_thread.is_none());
        assert!(record.event_gap);
        let events =
            serde_json::to_string(&f.store.execution_events(&f.actor, f.task, run).unwrap())
                .unwrap();
        assert!(events.contains("STARTUP_CONFIG_REPORTED"));
        assert!(!events.contains("private-service"));
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-SP03; config response failure cannot fall through to registration.
#[tokio::test]
async fn startup_configuration_failures_do_not_send_registration() {
    for (mode, expected) in [
        ("register-config-rejected", ErrorCode::AdapterRejected),
        ("register-config-wrong-id", ErrorCode::AdapterProtocol),
        ("register-config-mixed", ErrorCode::AdapterProtocol),
        ("register-config-malformed", ErrorCode::AdapterProtocol),
        ("register-config-missing", ErrorCode::AdapterProtocol),
        ("register-config-hang", ErrorCode::AdapterTimeout),
    ] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let run = session.binding().run_id;
        let source = Source::connect_with_file_tools(
            session,
            &mut f.store,
            &f.actor,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let result = source
            .start_file_thread(
                &mut f.store,
                &f.actor,
                Id::new_v4(),
                "baton-fixture",
                "baton_mock",
                if mode.ends_with("hang") {
                    Duration::from_millis(100)
                } else {
                    Duration::from_secs(5)
                },
            )
            .await;
        assert!(
            matches!(result, Err(ref error) if error.code == expected),
            "unexpected config error: {mode}"
        );
        assert!(f.temp.path().join("config-received").exists());
        assert!(!f.temp.path().join("registration-received").exists());
        let record = f.store.run(&f.actor, f.task, run).unwrap();
        assert!(record.source_thread.is_none());
        assert!(record.event_gap);
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-SP04; a reply must commit before registration; cancelled inspection never replays.
#[tokio::test]
async fn startup_configuration_commit_failure_and_cancellation_stop_scope() {
    for cancel in [false, true] {
        let mut f = Fixture::new();
        let session = f.launch(if cancel {
            "register-config-hang"
        } else {
            "register-good"
        });
        let run = session.binding().run_id;
        let source = Source::connect_with_file_tools(
            session,
            &mut f.store,
            &f.actor,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        if cancel {
            let read = source.start_file_thread(
                &mut f.store,
                &f.actor,
                Id::new_v4(),
                "baton-fixture",
                "baton_mock",
                Duration::from_secs(5),
            );
            assert!(tokio::time::timeout(Duration::from_millis(100), read)
                .await
                .is_err());
        } else {
            let connection =
                rusqlite::Connection::open(f.temp.path().join("state/baton.sqlite3")).unwrap();
            connection.execute_batch("CREATE TRIGGER refuse_config BEFORE INSERT ON execution_events WHEN json_extract(NEW.record,'$.observation.kind')='STARTUP_CONFIG_REPORTED' BEGIN SELECT RAISE(ABORT,'injected config failure'); END;").unwrap();
            let result = source
                .start_file_thread(
                    &mut f.store,
                    &f.actor,
                    Id::new_v4(),
                    "baton-fixture",
                    "baton_mock",
                    Duration::from_secs(5),
                )
                .await;
            assert!(matches!(result, Err(ref error) if error.code == ErrorCode::StoreFailure));
        }
        assert!(!f.temp.path().join("registration-received").exists());
        let record = f.store.run(&f.actor, f.task, run).unwrap();
        assert!(record.source_thread.is_none());
        assert!(record.event_gap);
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-R04; unsupported policy, lost replies and stable initialization never grant a thread.
#[tokio::test]
async fn failed_thread_registration_preserves_uncertainty_and_stops_scope() {
    for (mode, expected) in [
        ("register-disabled", ErrorCode::ExecutionNotEnabled),
        ("register-wrong-id", ErrorCode::AdapterProtocol),
        ("register-reject", ErrorCode::AdapterRejected),
        ("register-unsafe", ErrorCode::AdapterProtocol),
        ("register-hang", ErrorCode::AdapterTimeout),
    ] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let run = session.binding().run_id;
        let source = if mode == "register-disabled" {
            Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
                .await
                .unwrap()
                .with_file_creation()
        } else {
            Source::connect_with_file_tools(session, &mut f.store, &f.actor, Duration::from_secs(5))
                .await
                .unwrap()
        };
        let error = source
            .start_file_thread(
                &mut f.store,
                &f.actor,
                Id::new_v4(),
                "baton-fixture",
                "baton_mock",
                Duration::from_secs(5),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, expected, "{mode}");
        assert!(!error.to_string().contains("private vendor rejection"));
        let record = f.store.run(&f.actor, f.task, run).unwrap();
        assert!(record.source_thread.is_none());
        if mode == "register-disabled" {
            assert!(record.effects.is_empty());
            assert!(!f.temp.path().join("registration-received").exists());
        } else {
            assert!(
                f.temp.path().join("registration-received").exists(),
                "{mode}"
            );
            assert!(
                record
                    .effects
                    .iter()
                    .any(|effect| effect.class == EffectClass::Unknown
                        && effect.state == EffectState::Uncertain),
                "{mode}"
            );
        }
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-R04; a reply whose metadata cannot commit is not a registered thread fact.
#[tokio::test]
async fn thread_registration_storage_failure_keeps_unsettled_startup() {
    let mut f = Fixture::new();
    let session = f.launch("register-good");
    let run = session.binding().run_id;
    let source =
        Source::connect_with_file_tools(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
    let connection = rusqlite::Connection::open(f.temp.path().join("state/baton.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER refuse_registration BEFORE INSERT ON execution_events WHEN json_extract(NEW.record,'$.observation.kind')='THREAD_REGISTERED' BEGIN SELECT RAISE(ABORT,'injected registration failure'); END;").unwrap();
    assert_eq!(
        source
            .start_file_thread(
                &mut f.store,
                &f.actor,
                Id::new_v4(),
                "baton-fixture",
                "baton_mock",
                Duration::from_secs(5)
            )
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::StoreFailure
    );
    let record = f.store.run(&f.actor, f.task, run).unwrap();
    assert!(record.source_thread.is_none());
    assert!(record.effects.iter().any(
        |effect| effect.class == EffectClass::Unknown && effect.state == EffectState::Uncertain
    ));
    assert!(f.temp.path().join("registration-received").exists());
    #[cfg(windows)]
    f.assert_child_stopped().await;
}

// M5-R04; retained registration is bound to its journal, not mutable run JSON.
#[tokio::test]
async fn corrupted_thread_registration_is_preserved_and_refused() {
    for target in ["run", "journal"] {
        let mut f = Fixture::new();
        let session = f.launch("register-good");
        let run = session.binding().run_id;
        let source = Source::connect_with_file_tools(
            session,
            &mut f.store,
            &f.actor,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let (source, _) = source
            .start_file_thread(
                &mut f.store,
                &f.actor,
                Id::new_v4(),
                "baton-fixture",
                "baton_mock",
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        let connection =
            rusqlite::Connection::open(f.temp.path().join("state/baton.sqlite3")).unwrap();
        if target == "run" {
            connection.execute("UPDATE runs SET record=json_set(record,'$.source_thread','forged') WHERE id=?1", [run.to_string()]).unwrap();
        } else {
            assert!(connection.execute("UPDATE execution_events SET record=json_set(record,'$.observation.thread_id','forged') WHERE run_id=?1 AND json_extract(record,'$.observation.kind')='THREAD_REGISTERED'", [run.to_string()]).is_err());
            connection
                .execute_batch("DROP TRIGGER execution_no_update")
                .unwrap();
            connection.execute("UPDATE execution_events SET record=json_set(record,'$.observation.thread_id','forged') WHERE run_id=?1 AND json_extract(record,'$.observation.kind')='THREAD_REGISTERED'", [run.to_string()]).unwrap();
        }
        assert_eq!(
            f.store.run(&f.actor, f.task, run).unwrap_err().code,
            ErrorCode::CorruptStore
        );
        assert_eq!(
            source
                .stop(&mut f.store, &f.actor, Duration::from_secs(5))
                .unwrap_err()
                .code,
            ErrorCode::CorruptStore
        );
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-SG01; FR-04/06/12/14, INV-04/09: mutable summaries cannot settle journaled effects.
#[tokio::test]
async fn startup_gate_refuses_corrupt_effect_summaries_and_observations() {
    for target in ["effect", "cursor", "observation", "sequence", "reseal"] {
        let mut f = Fixture::new();
        let session = f.launch("register-create");
        let run = session.binding().run_id;
        let source = Source::connect_with_file_tools(
            session,
            &mut f.store,
            &f.actor,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let (source, _) = source
            .start_file_thread(
                &mut f.store,
                &f.actor,
                Id::new_v4(),
                "baton-fixture",
                "baton_mock",
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        let connection =
            rusqlite::Connection::open(f.temp.path().join("state/baton.sqlite3")).unwrap();
        match target {
            "effect" => {
                connection.execute("UPDATE runs SET record=json_set(record,'$.effects[2].state','COMPLETED','$.effects[2].class','LOCAL_REVERSIBLE') WHERE id=?1", [run.to_string()]).unwrap();
            }
            "cursor" => {
                connection
                    .execute(
                        "UPDATE runs SET record=json_set(record,'$.source_cursor',999) WHERE id=?1",
                        [run.to_string()],
                    )
                    .unwrap();
            }
            "observation" => {
                connection
                    .execute_batch("DROP TRIGGER execution_no_update")
                    .unwrap();
                connection.execute("UPDATE execution_events SET record=json_set(record,'$.observation.class','LOCAL_REVERSIBLE') WHERE run_id=?1 AND json_extract(record,'$.observation.kind')='TOOL_STARTED' AND json_extract(record,'$.observation.class')='UNKNOWN'", [run.to_string()]).unwrap();
            }
            "sequence" => {
                connection
                    .execute_batch("DROP TRIGGER execution_no_update")
                    .unwrap();
                connection.execute("UPDATE execution_events SET record=(SELECT record FROM execution_events WHERE run_id=?1 AND sequence=1) WHERE run_id=?1 AND sequence=2", [run.to_string()]).unwrap();
            }
            _ => {
                connection.execute("UPDATE runs SET record=json_set(record,'$.effects[2].state','COMPLETED','$.effects[2].class','LOCAL_REVERSIBLE') WHERE id=?1", [run.to_string()]).unwrap();
                let json: String = connection
                    .query_row(
                        "SELECT record FROM runs WHERE id=?1",
                        [run.to_string()],
                        |row| row.get(0),
                    )
                    .unwrap();
                let forged: RunRecord = serde_json::from_str(&json).unwrap();
                let hash = blake3::hash(&serde_json::to_vec(&forged).unwrap())
                    .to_hex()
                    .to_string();
                connection
                    .execute_batch("DROP TRIGGER execution_no_update")
                    .unwrap();
                connection.execute("UPDATE execution_events SET record=json_set(record,'$.state_hash',?1) WHERE run_id=?2 AND sequence=(SELECT max(sequence) FROM execution_events WHERE run_id=?2)", rusqlite::params![hash,run.to_string()]).unwrap();
            }
        }
        assert_eq!(
            f.store.run(&f.actor, f.task, run).unwrap_err().code,
            ErrorCode::CorruptStore,
            "{target}"
        );
        assert_eq!(
            source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-registered",
                    Duration::from_secs(5)
                )
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::CorruptStore
        );
        assert!(!f.repo.path().join("created.txt").exists());
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-SG02; historical replies remain valid, but edited cached facts must fail closed.
#[tokio::test]
async fn historical_execution_reply_is_checked_against_its_recorded_state() {
    let mut f = Fixture::new();
    let session = f.launch("register-after");
    let run = session.binding().run_id;
    let source =
        Source::connect_with_file_tools(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
    let (source, original) = source
        .start_file_thread(
            &mut f.store,
            &f.actor,
            Id::new_v4(),
            "baton-fixture",
            "baton_mock",
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    let event = f
        .store
        .execution_events(&f.actor, f.task, run)
        .unwrap()
        .pop()
        .unwrap();
    let (source, newer) = source
        .capture_next(
            &mut f.store,
            &f.actor,
            &adapter(),
            "thread-registered",
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert!(newer.source_cursor > original.source_cursor);
    let retry = Request {
        schema_version: SchemaVersion::V0_1_0,
        operation_id: event.operation_id,
        actor_id: f.actor.id(),
        expected_epoch: event.binding.epoch,
        action: Action::ObserveRun {
            task_id: f.task,
            run_id: run,
            lease_id: f.lease.lease_id,
            observation: event.observation,
        },
    };
    assert_eq!(
        f.store.execute(&f.actor, &retry).unwrap(),
        Reply::Run(original.clone())
    );
    let connection = rusqlite::Connection::open(f.temp.path().join("state/baton.sqlite3")).unwrap();
    let mut forged = original;
    forged.effects[2].state = EffectState::Completed;
    forged.effects[2].class = EffectClass::LocalReversible;
    let response =
        serde_json::to_string(&Result::<Reply, Diagnostic>::Ok(Reply::Run(forged))).unwrap();
    connection
        .execute_batch("DROP TRIGGER operations_no_update")
        .unwrap();
    connection
        .execute(
            "UPDATE operations SET result=?1 WHERE id=?2",
            rusqlite::params![response, event.operation_id.to_string()],
        )
        .unwrap();
    assert_eq!(
        f.store.execute(&f.actor, &retry).unwrap_err().code,
        ErrorCode::CorruptStore
    );
    assert_eq!(f.store.run(&f.actor, f.task, run).unwrap(), newer);
    source
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
}

// M5-R04; registration does not settle unknown startup effects or admit writes.
#[tokio::test]
async fn registered_thread_cannot_bypass_unresolved_startup_effects() {
    let mut f = Fixture::new();
    let session = f.launch("register-create");
    let run = session.binding().run_id;
    let source =
        Source::connect_with_file_tools(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
    let (mut source, _) = source
        .start_file_thread(
            &mut f.store,
            &f.actor,
            Id::new_v4(),
            "baton-fixture",
            "baton_mock",
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    for _ in 0..2 {
        source = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-registered",
                Duration::from_secs(5),
            )
            .await
            .unwrap()
            .0;
    }
    let error = source
        .capture_next(
            &mut f.store,
            &f.actor,
            &adapter(),
            "thread-registered",
            Duration::from_secs(5),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(!f.repo.path().join("created.txt").exists());
    assert!(f
        .store
        .run(&f.actor, f.task, run)
        .unwrap()
        .blockers()
        .contains(&BoundaryReason::UnresolvedEffect));
    #[cfg(windows)]
    f.assert_child_stopped().await;
}

// M5-X03; FR-03/06/14/15, INV-04/05/07/09: host completion precedes a positive reply.
#[tokio::test]
async fn opted_in_callback_creates_a_file_and_records_distinct_phases() {
    let mut f = Fixture::new();
    let session = f.launch("create-file");
    let run = session.binding().run_id;
    let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap()
        .with_file_creation();
    for _ in 0..6 {
        let (next, record) = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert!(!record.event_gap);
        source = next;
    }
    assert_eq!(
        std::fs::read(f.repo.path().join("created.txt")).unwrap(),
        b"private created content"
    );
    assert!(f.temp.path().join("creation-acknowledged").exists());
    let events = f.store.execution_events(&f.actor, f.task, run).unwrap();
    let phases: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.observation {
            RunObservation::AdapterReported {
                event:
                    AdapterEvent {
                        event: AdapterEventKind::DynamicToolExecution(callback),
                        ..
                    },
                ..
            } => Some((callback.phase, e.origin)),
            _ => None,
        })
        .collect();
    assert_eq!(
        phases,
        vec![
            (CallbackPhase::Planned, ExecutionOrigin::HostReported),
            (CallbackPhase::Written, ExecutionOrigin::HostReported),
            (CallbackPhase::Resolved, ExecutionOrigin::AdapterObserved)
        ]
    );
    let record = f.store.run(&f.actor, f.task, run).unwrap();
    assert_eq!(record.effects.len(), 1);
    assert_eq!(record.effects[0].state, EffectState::Completed);
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("private created content"));
    source
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
}

// M5-X04; repeated or changed RPC identities never replay a completed operation.
#[tokio::test]
async fn file_callbacks_cannot_replay_or_rebind_an_executed_request() {
    for mode in ["create-duplicate", "create-changed", "create-reuse"] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let run = session.binding().run_id;
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap()
            .with_file_creation();
        for _ in 0..if mode == "create-reuse" { 6 } else { 3 } {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        if mode == "create-duplicate" {
            std::fs::remove_file(f.repo.path().join("created.txt")).unwrap();
        }
        let error = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::IdempotencyConflict);
        if mode == "create-duplicate" {
            assert!(!f.repo.path().join("created.txt").exists());
        } else {
            assert_eq!(
                std::fs::read(f.repo.path().join("created.txt")).unwrap(),
                b"private created content"
            );
        }
        let record = f.store.run(&f.actor, f.task, run).unwrap();
        assert_eq!(record.effects.len(), 1);
        assert_eq!(record.effects[0].state, EffectState::Completed);
        assert!(record.event_gap);
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-X05; default denial and create_new both preserve existing workspace contents.
#[tokio::test]
async fn file_callbacks_require_opt_in_and_preserve_existing_files() {
    for enabled in [false, true] {
        let mut f = Fixture::new();
        if enabled {
            std::fs::write(f.repo.path().join("created.txt"), b"preserve").unwrap();
        }
        let session = f.launch(if enabled {
            "create-file"
        } else {
            "create-disabled"
        });
        let run = session.binding().run_id;
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        if enabled {
            source = source.with_file_creation();
        }
        for _ in 0..2 {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        let result = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await;
        if enabled {
            assert_eq!(result.err().unwrap().code, ErrorCode::Io);
            assert_eq!(
                std::fs::read(f.repo.path().join("created.txt")).unwrap(),
                b"preserve"
            );
            let record = f.store.run(&f.actor, f.task, run).unwrap();
            assert_eq!(record.effects[0].state, EffectState::Uncertain);
            #[cfg(windows)]
            f.assert_child_stopped().await;
        } else {
            let source = result.unwrap().0;
            assert!(!f.repo.path().join("created.txt").exists());
            assert!(f
                .store
                .run(&f.actor, f.task, run)
                .unwrap()
                .effects
                .is_empty());
            source
                .stop(&mut f.store, &f.actor, Duration::from_secs(5))
                .unwrap();
        }
        assert!(!f.temp.path().join("creation-acknowledged").exists());
    }
}

// M5-X06; loss around completion/response commits preserves only observed facts.
#[tokio::test]
async fn file_callback_storage_failures_preserve_effects_without_replay() {
    for phase in ["PLANNED", "TOOL_FINISHED", "WRITTEN"] {
        let mut f = Fixture::new();
        let session = f.launch("create-file");
        let run = session.binding().run_id;
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap()
            .with_file_creation();
        for _ in 0..2 {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        let connection =
            rusqlite::Connection::open(f.temp.path().join("state/baton.sqlite3")).unwrap();
        let condition = if phase == "TOOL_FINISHED" {
            "json_extract(NEW.record,'$.observation.kind')='TOOL_FINISHED'".to_owned()
        } else {
            format!("json_extract(NEW.record,'$.observation.event.event.kind')='DYNAMIC_TOOL_EXECUTION' AND json_extract(NEW.record,'$.observation.event.event.phase')='{phase}'")
        };
        connection.execute_batch(&format!("CREATE TRIGGER refuse_test_phase BEFORE INSERT ON execution_events WHEN {condition} BEGIN SELECT RAISE(ABORT,'injected storage failure'); END;")).unwrap();
        let error = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::StoreFailure, "{phase}");
        let record = f.store.run(&f.actor, f.task, run).unwrap();
        if phase == "PLANNED" {
            assert!(!f.repo.path().join("created.txt").exists());
            assert!(record.effects.is_empty());
        } else {
            assert_eq!(
                std::fs::read(f.repo.path().join("created.txt")).unwrap(),
                b"private created content"
            );
            assert_eq!(
                record.effects[0].state,
                if phase == "WRITTEN" {
                    EffectState::Completed
                } else {
                    EffectState::Uncertain
                }
            );
        }
        let events = f.store.execution_events(&f.actor, f.task, run).unwrap();
        assert!(!events.iter().any(|e| matches!(
            &e.observation,
            RunObservation::AdapterReported {
                event: AdapterEvent {
                    event: AdapterEventKind::DynamicToolExecution(callback),
                    ..
                },
                ..
            } if matches!(callback.phase, CallbackPhase::Written | CallbackPhase::Resolved)
        )));
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-D03; FR-03/06/14/15, INV-04/07/09: real pipes carry failure, not an execution grant.
#[tokio::test]
async fn dynamic_callback_is_refused_without_executing_or_inventing_effects() {
    let mut f = Fixture::new();
    let session = f.launch("callback-dynamic");
    let run = session.binding().run_id;
    let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    for _ in 0..6 {
        let (next, record) = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert!(!record.event_gap);
        assert!(record.effects.is_empty());
        source = next;
    }
    assert!(f.temp.path().join("callback-refused").exists());
    let events = f.store.execution_events(&f.actor, f.task, run).unwrap();
    let phases: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.observation {
            RunObservation::AdapterReported {
                event:
                    AdapterEvent {
                        event: AdapterEventKind::CallbackRefusal { phase, .. },
                        ..
                    },
                ..
            } => Some((*phase, e.origin)),
            _ => None,
        })
        .collect();
    assert_eq!(
        phases,
        vec![
            (CallbackPhase::Planned, ExecutionOrigin::HostReported),
            (CallbackPhase::Written, ExecutionOrigin::HostReported),
            (CallbackPhase::Resolved, ExecutionOrigin::AdapterObserved)
        ]
    );
    let json = serde_json::to_string(&events).unwrap();
    assert!(!json.contains("private arguments"));
    source
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
}

// M5-U03; FR-03/06/12/15, INV-04/09: record input-required, stop scope, never synthesize an answer.
#[tokio::test]
async fn user_input_request_blocks_without_answering_the_user() {
    let mut f = Fixture::new();
    let session = f.launch("callback-input");
    let run = session.binding().run_id;
    let source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    let source = source
        .capture_next(
            &mut f.store,
            &f.actor,
            &adapter(),
            "thread-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap()
        .0;
    let error = source
        .capture_next(
            &mut f.store,
            &f.actor,
            &adapter(),
            "thread-1",
            Duration::from_secs(5),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::ExecutionNotEnabled);
    let record = f.store.run(&f.actor, f.task, run).unwrap();
    assert!(record.event_gap);
    assert_eq!(record.state, RunState::RecoveryRequired);
    let events = f.store.execution_events(&f.actor, f.task, run).unwrap();
    assert!(events.iter().any(|e| matches!(
        &e.observation,
        RunObservation::AdapterReported {
            event: AdapterEvent {
                event: AdapterEventKind::CallbackRefusal {
                    callback: CallbackKind::UserInput,
                    phase: CallbackPhase::Planned,
                    ..
                },
                ..
            },
            ..
        }
    )));
    let json = serde_json::to_string(&events).unwrap();
    assert!(!json.contains("private question"));
    assert!(!json.contains("WRITTEN"));
    #[cfg(windows)]
    f.assert_child_stopped().await;
    assert!(!f.temp.path().join("invented-answer").exists());
}

// M5-D04; FR-03/12/15, INV-07/09: changed payload and resolved-ID reuse cannot create a second action.
#[tokio::test]
async fn callback_payload_conflicts_and_resolved_reuse_stop_capture() {
    for (mode, captures) in [("callback-changed", 3), ("callback-reuse", 6)] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let run = session.binding().run_id;
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        for _ in 0..captures {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        let error = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::IdempotencyConflict);
        let record = f.store.run(&f.actor, f.task, run).unwrap();
        assert!(record.event_gap);
        assert!(record.effects.is_empty());
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-D05; FR-03/12, INV-04/09: keep only committed refusal phases if storage fails.
#[tokio::test]
async fn callback_storage_failure_retains_committed_intent_only() {
    use std::sync::atomic::{AtomicI64, Ordering};
    for allowed in [0, 1] {
        let clock = Arc::new(FailingClock(AtomicI64::new(-1)));
        let mut f = Fixture::with_clock(clock.clone());
        let session = f.launch("callback-dynamic");
        let run = session.binding().run_id;
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        for _ in 0..2 {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        clock.0.store(allowed, Ordering::SeqCst);
        let error = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::StoreFailure);
        let json = serde_json::to_string(&f.store.execution_events(&f.actor, f.task, run).unwrap())
            .unwrap();
        assert_eq!(json.matches("CALLBACK_REFUSAL").count(), allowed as usize);
        assert!(!json.contains("WRITTEN"));
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-I02; FR-03/06/12/15, INV-04/06/07/09: real pipes, interleaving and retry without replay.
#[tokio::test]
async fn interrupt_reply_and_terminal_turn_never_replace_native_drain() {
    for mode in ["interrupt-normal", "interrupt-completed-first"] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let run_id = session.binding().run_id;
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        for _ in 0..2 {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        let (next, record) = source
            .request_interrupt(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                "turn-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(record.state, RunState::Stopping);
        assert!(!record.tree_stopped);
        // Same live request cannot send a second cancellation or reconstruct a permit.
        source = next
            .request_interrupt(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                "turn-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap()
            .0;
        for _ in 0..3 {
            let (next, record) = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap();
            assert_eq!(record.state, RunState::Stopping);
            assert!(!record.tree_stopped);
            assert!(!record.event_gap);
            source = next;
        }
        let journal = f.store.execution_events(&f.actor, f.task, run_id).unwrap();
        let phases: Vec<_> = journal
            .iter()
            .filter_map(|entry| {
                if let RunObservation::AdapterReported { event, .. } = &entry.observation {
                    match event.event {
                        AdapterEventKind::TurnInterruptPlanned { .. } => Some((0, entry.origin)),
                        AdapterEventKind::TurnInterruptWritten { .. } => Some((1, entry.origin)),
                        AdapterEventKind::TurnInterruptAcknowledged { .. } => {
                            Some((2, entry.origin))
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            phases,
            vec![
                (0, ExecutionOrigin::HostReported),
                (1, ExecutionOrigin::HostReported),
                (2, ExecutionOrigin::AdapterObserved)
            ]
        );
        let stopped = source
            .stop(&mut f.store, &f.actor, Duration::from_secs(5))
            .unwrap();
        assert_eq!(stopped.effects[0].state, EffectState::Uncertain);
        assert!(stopped
            .blockers()
            .contains(&BoundaryReason::UnresolvedEffect));
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-I04; FR-03/06/12/15, INV-06/09: uncorrelated/rejected/missing replies preserve uncertainty.
#[tokio::test]
async fn interrupt_failures_close_scope_without_claiming_a_boundary() {
    for (mode, code) in [
        ("interrupt-wrong-id", ErrorCode::AdapterProtocol),
        ("interrupt-rejected", ErrorCode::AdapterRejected),
        ("interrupt-no-reply", ErrorCode::AdapterTimeout),
    ] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let run = session.binding().run_id;
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        for _ in 0..2 {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        source = source
            .request_interrupt(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                "turn-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap()
            .0;
        let diagnostic = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_millis(250),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(diagnostic.code, code);
        let record = f.store.run(&f.actor, f.task, run).unwrap();
        assert_eq!(record.state, RunState::RecoveryRequired);
        assert!(record.event_gap);
        assert!(!record.tree_stopped);
        assert_eq!(record.effects[0].state, EffectState::Uncertain);
        let json = serde_json::to_string(&f.store.execution_events(&f.actor, f.task, run).unwrap())
            .unwrap();
        assert!(!json.contains("private rejection"));
        assert!(!json.contains("TURN_INTERRUPT_ACKNOWLEDGED"));
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-I05; FR-03/12, INV-04/09: failed intent cannot send; failed postwrite audit is ambiguous.
#[tokio::test]
async fn interrupt_storage_failure_retains_only_committed_phases() {
    use std::sync::atomic::{AtomicI64, Ordering};
    for allowed in [0, 1] {
        let clock = Arc::new(FailingClock(AtomicI64::new(-1)));
        let mut f = Fixture::with_clock(clock.clone());
        let session = f.launch("interrupt-normal");
        let run = session.binding().run_id;
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        for _ in 0..2 {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        clock.0.store(allowed, Ordering::SeqCst);
        let diagnostic = source
            .request_interrupt(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                "turn-1",
                Duration::from_secs(5),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(diagnostic.code, ErrorCode::StoreFailure);
        let json = serde_json::to_string(&f.store.execution_events(&f.actor, f.task, run).unwrap())
            .unwrap();
        assert_eq!(
            json.matches("TURN_INTERRUPT_PLANNED").count(),
            allowed as usize
        );
        assert!(!json.contains("TURN_INTERRUPT_WRITTEN"));
        assert!(!json.contains("TURN_INTERRUPT_ACKNOWLEDGED"));
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-I06; FR-03/15, INV-07/09: wrong turn is refused before sending control data.
#[tokio::test]
async fn interrupt_wrong_turn_never_reaches_the_peer() {
    let mut f = Fixture::new();
    let session = f.launch("interrupt-normal");
    let run = session.binding().run_id;
    let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    for _ in 0..2 {
        source = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap()
            .0;
    }
    let diagnostic = source
        .request_interrupt(
            &mut f.store,
            &f.actor,
            &adapter(),
            "thread-1",
            "another-turn",
            Duration::from_secs(5),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(diagnostic.code, ErrorCode::InvalidInput);
    #[cfg(windows)]
    f.assert_child_stopped().await;
    assert!(!f.temp.path().join("interrupt-received").exists());
    assert!(f.store.run(&f.actor, f.task, run).unwrap().event_gap);
}

// M5-A03; real owned pipe cancellation, durable intent before written response, no execution grant.
#[tokio::test]
async fn approvals_are_cancelled_with_distinct_host_and_vendor_evidence() {
    for mode in ["approval-numeric", "approval-text", "approval-file"] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let binding = session.binding().clone();
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        for _ in 0..6 {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        let events = f
            .store
            .execution_events(&f.actor, f.task, binding.run_id)
            .unwrap();
        let planned = events
            .iter()
            .find(|entry| {
                matches!(
                    &entry.observation,
                    RunObservation::AdapterReported {
                        event: AdapterEvent {
                            event: AdapterEventKind::ApprovalCancellationPlanned { .. },
                            ..
                        },
                        ..
                    }
                )
            })
            .unwrap();
        let written = events
            .iter()
            .find(|entry| {
                matches!(
                    &entry.observation,
                    RunObservation::AdapterReported {
                        event: AdapterEvent {
                            event: AdapterEventKind::ApprovalCancellationWritten { .. },
                            ..
                        },
                        ..
                    }
                )
            })
            .unwrap();
        let resolved = events
            .iter()
            .find(|entry| {
                matches!(
                    &entry.observation,
                    RunObservation::AdapterReported {
                        event: AdapterEvent {
                            event: AdapterEventKind::ApprovalResolutionReported { .. },
                            ..
                        },
                        ..
                    }
                )
            })
            .unwrap();
        assert!(planned.sequence < written.sequence && written.sequence < resolved.sequence);
        assert_eq!(planned.origin, ExecutionOrigin::HostReported);
        assert_eq!(written.origin, ExecutionOrigin::HostReported);
        assert_eq!(resolved.origin, ExecutionOrigin::AdapterObserved);
        let run = f.store.run(&f.actor, f.task, binding.run_id).unwrap();
        assert!(!run.event_gap);
        assert_eq!(run.effects[0].state, EffectState::Uncertain);
        assert!(!serde_json::to_string(&events).unwrap().contains("private"));
        assert_eq!(
            std::fs::read(f.temp.path().join("cancel-reply")).unwrap(),
            b"cancel"
        );
        assert!(!f.temp.path().join("dangerous-work").exists());
        source
            .stop(&mut f.store, &f.actor, Duration::from_secs(5))
            .unwrap();
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

struct FailingClock(std::sync::atomic::AtomicI64);
impl baton_store::Clock for FailingClock {
    fn now_ms(&self) -> Result<i64, Diagnostic> {
        use std::sync::atomic::Ordering;
        let remaining = self.0.load(Ordering::SeqCst);
        if remaining == 0 {
            return Err(Diagnostic::new(ErrorCode::StoreFailure));
        }
        if remaining > 0 {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(1000)
    }
}
// M5-A04; intent is durable before transport; absent written evidence never implies nondelivery.
#[tokio::test]
async fn storage_failure_retains_only_committed_cancellation_phases() {
    use std::sync::atomic::{AtomicI64, Ordering};
    for allowed in [0, 1] {
        let clock = Arc::new(FailingClock(AtomicI64::new(-1)));
        let mut f = Fixture::with_clock(clock.clone());
        let session = f.launch("approval-numeric");
        let binding = session.binding().clone();
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        for _ in 0..2 {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        clock.0.store(allowed, Ordering::SeqCst);
        let error = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::StoreFailure);
        let events = f
            .store
            .execution_events(&f.actor, f.task, binding.run_id)
            .unwrap();
        let plans = events
            .iter()
            .filter(|event| {
                matches!(
                    &event.observation,
                    RunObservation::AdapterReported {
                        event: AdapterEvent {
                            event: AdapterEventKind::ApprovalCancellationPlanned { .. },
                            ..
                        },
                        ..
                    }
                )
            })
            .count();
        assert_eq!(plans, allowed as usize);
        assert!(!events.iter().any(|event| matches!(
            &event.observation,
            RunObservation::AdapterReported {
                event: AdapterEvent {
                    event: AdapterEventKind::ApprovalCancellationWritten { .. },
                    ..
                },
                ..
            }
        )));
        assert!(!f.temp.path().join("dangerous-work").exists());
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-A05; absent cancellation support and unrecognized permission requests never fall back to accept.
#[tokio::test]
async fn unsupported_approval_requests_close_scope_without_a_grant() {
    for mode in ["approval-no-cancel", "approval-unsupported"] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let binding = session.binding().clone();
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        for _ in 0..2 {
            source = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap()
                .0;
        }
        assert_eq!(
            source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5)
                )
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::AdapterProtocol
        );
        assert_eq!(
            f.store.run(&f.actor, f.task, binding.run_id).unwrap().state,
            RunState::RecoveryRequired
        );
        assert!(!f.temp.path().join("cancel-reply").exists());
        assert!(!f.temp.path().join("dangerous-work").exists());
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-A06; cancelling pending work remains possible during source freeze and never changes epoch.
#[tokio::test]
async fn cancellation_during_quiescing_does_not_revive_a_writer() {
    let mut f = Fixture::new();
    let session = f.launch("approval-numeric");
    let binding = session.binding().clone();
    let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    for _ in 0..2 {
        source = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap()
            .0;
    }
    f.store
        .execute(
            &f.actor,
            &Request {
                schema_version: SchemaVersion::V0_1_0,
                operation_id: Id::new_v4(),
                actor_id: f.actor.id(),
                expected_epoch: 1,
                action: Action::RequestTransfer {
                    task_id: f.task,
                    lease_id: binding.lease_id,
                    transfer_id: Id::new_v4(),
                    receiver_id: f.actor.id(),
                },
            },
        )
        .unwrap();
    for _ in 0..4 {
        source = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap()
            .0;
    }
    let task = f.store.task(&f.actor, f.task).unwrap();
    assert_eq!(task.state, TaskState::Quiescing);
    assert_eq!(task.epoch, 1);
    assert_eq!(
        f.store
            .run(&f.actor, f.task, binding.run_id)
            .unwrap()
            .effects[0]
            .state,
        EffectState::Uncertain
    );
    source
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    #[cfg(windows)]
    f.assert_child_stopped().await;
}

// M5-A07; ambiguous reuse of a resolved RPC ID cannot silently create another control operation.
#[tokio::test]
async fn resolved_request_id_reuse_is_refused_without_a_second_reply() {
    let mut f = Fixture::new();
    let session = f.launch("approval-reuse");
    let binding = session.binding().clone();
    let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    for _ in 0..6 {
        source = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap()
            .0;
    }
    assert_eq!(
        source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5)
            )
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::IdempotencyConflict
    );
    let run = f.store.run(&f.actor, f.task, binding.run_id).unwrap();
    assert!(run.event_gap);
    assert_eq!(run.state, RunState::RecoveryRequired);
    let events = f
        .store
        .execution_events(&f.actor, f.task, binding.run_id)
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|entry| matches!(
                &entry.observation,
                RunObservation::AdapterReported {
                    event: AdapterEvent {
                        event: AdapterEventKind::ApprovalCancellationWritten { .. },
                        ..
                    },
                    ..
                }
            ))
            .count(),
        1
    );
    #[cfg(windows)]
    f.assert_child_stopped().await;
}

// M5-P02; all supported passive item types preserve lifecycle, omit contents and create no tool effect.
#[tokio::test]
async fn passive_lifecycle_is_retained_without_text_or_effects() {
    let mut f = Fixture::new();
    let session = f.launch("passive");
    let binding = session.binding().clone();
    let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    for _ in 0..12 {
        source = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap()
            .0;
    }
    let run = f.store.run(&f.actor, f.task, binding.run_id).unwrap();
    assert!(!run.event_gap);
    assert!(run.effects.is_empty());
    let events = f
        .store
        .execution_events(&f.actor, f.task, binding.run_id)
        .unwrap();
    assert!(!serde_json::to_string(&events).unwrap().contains("private"));
    source
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    #[cfg(windows)]
    f.assert_child_stopped().await;
}

// M5-E06; INV-02/06/09: a new vendor turn after QUIESCING is retained and forces recovery.
#[tokio::test]
async fn adapter_turn_after_quiescing_cannot_bypass_source_freeze() {
    let mut f = Fixture::new();
    let session = f.launch("trace");
    let binding = session.binding().clone();
    let source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    f.store
        .execute(
            &f.actor,
            &Request {
                schema_version: SchemaVersion::V0_1_0,
                operation_id: Id::new_v4(),
                actor_id: f.actor.id(),
                expected_epoch: 1,
                action: Action::RequestTransfer {
                    task_id: f.task,
                    lease_id: binding.lease_id,
                    transfer_id: Id::new_v4(),
                    receiver_id: f.actor.id(),
                },
            },
        )
        .unwrap();
    assert_eq!(
        f.store.task(&f.actor, f.task).unwrap().state,
        TaskState::Quiescing
    );
    let (source, _) = source
        .capture_next(
            &mut f.store,
            &f.actor,
            &adapter(),
            "thread-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(
        f.store.task(&f.actor, f.task).unwrap().state,
        TaskState::Suspect
    );
    let run = f.store.run(&f.actor, f.task, binding.run_id).unwrap();
    assert_eq!(run.state, RunState::RecoveryRequired);
    assert!(run.event_gap);
    assert!(f
        .store
        .execution_events(&f.actor, f.task, binding.run_id)
        .unwrap()
        .iter()
        .any(|event| matches!(
            &event.observation,
            RunObservation::AdapterReported {
                event: AdapterEvent {
                    event: AdapterEventKind::TurnStarted,
                    ..
                },
                ..
            }
        )));
    drop(source);
    #[cfg(windows)]
    f.assert_child_stopped().await;
}
// M5-E01/E02/E03; actual native stream, durable correlation, duplicate retries and redacted facts.
#[tokio::test]
async fn native_stream_is_durable_idempotent_and_retains_unknown_effects() {
    let mut f = Fixture::new();
    let session = f.launch("trace");
    let binding = session.binding().clone();
    let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    for expected in [1, 2, 2, 3, 4] {
        let (next, record) = source
            .capture_next(
                &mut f.store,
                &f.actor,
                &adapter(),
                "thread-1",
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(record.source_cursor, expected);
        source = next;
    }
    let events = f
        .store
        .execution_events(&f.actor, f.task, binding.run_id)
        .unwrap();
    let adapter_events: Vec<_> = events
        .iter()
        .filter(|event| matches!(event.observation, RunObservation::AdapterReported { .. }))
        .collect();
    assert_eq!(adapter_events.len(), 4);
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains("private"));
    let run = f.store.run(&f.actor, f.task, binding.run_id).unwrap();
    assert_eq!(run.effects[0].state, EffectState::Uncertain);
    assert!(!run.event_gap);
    assert!(!run.tree_stopped);
    let cached = adapter_events[1];
    let retry = Request {
        schema_version: SchemaVersion::V0_1_0,
        operation_id: cached.operation_id,
        actor_id: f.actor.id(),
        expected_epoch: binding.epoch,
        action: Action::ObserveRun {
            task_id: f.task,
            run_id: binding.run_id,
            lease_id: binding.lease_id,
            observation: cached.observation.clone(),
        },
    };
    let mut reopened = Store::open(&f.temp.path().join("state"), Arc::new(SystemClock)).unwrap();
    assert!(reopened.execute(&f.actor, &retry).is_ok());
    assert_eq!(
        reopened
            .execution_events(&f.actor, f.task, binding.run_id)
            .unwrap()
            .len(),
        events.len()
    );
    source
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    #[cfg(windows)]
    f.assert_child_stopped().await;
}

// M5-E02; orphan facts are retained, contradictory retries and foreign streams are blocked.
#[tokio::test]
async fn stream_gaps_conflicts_and_wrong_binding_never_become_safe_completion() {
    for mode in ["orphan", "foreign-thread", "changed-duplicate"] {
        let mut f = Fixture::new();
        let session = f.launch(mode);
        let binding = session.binding().clone();
        let mut source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        if mode == "orphan" {
            let (source, record) = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .unwrap();
            assert!(record.event_gap);
            assert_eq!(record.effects[0].state, EffectState::Uncertain);
            source
                .stop(&mut f.store, &f.actor, Duration::from_secs(5))
                .unwrap();
        } else {
            if mode == "changed-duplicate" {
                for _ in 0..2 {
                    source = source
                        .capture_next(
                            &mut f.store,
                            &f.actor,
                            &adapter(),
                            "thread-1",
                            Duration::from_secs(5),
                        )
                        .await
                        .unwrap()
                        .0;
                }
            }
            let error = source
                .capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(5),
                )
                .await
                .err()
                .unwrap();
            assert_eq!(
                error.code,
                if mode == "foreign-thread" {
                    ErrorCode::AdapterProtocol
                } else {
                    ErrorCode::IdempotencyConflict
                }
            );
            let run = f.store.run(&f.actor, f.task, binding.run_id).unwrap();
            assert!(run.event_gap);
            assert_eq!(run.state, RunState::RecoveryRequired);
        }
        let events = f
            .store
            .execution_events(&f.actor, f.task, binding.run_id)
            .unwrap();
        assert!(!serde_json::to_string(&events)
            .unwrap()
            .contains("foreign-thread"));
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-E04; corrupt adapter evidence refuses reads and exact retries, preserving files.
#[tokio::test]
async fn damaged_adapter_evidence_blocks_reads_and_cached_acknowledgements() {
    let mut f = Fixture::new();
    let session = f.launch("trace");
    let binding = session.binding().clone();
    let source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
        .await
        .unwrap();
    let (source, _) = source
        .capture_next(
            &mut f.store,
            &f.actor,
            &adapter(),
            "thread-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    let events = f
        .store
        .execution_events(&f.actor, f.task, binding.run_id)
        .unwrap();
    let event = events.last().unwrap();
    let request = Request {
        schema_version: SchemaVersion::V0_1_0,
        operation_id: event.operation_id,
        actor_id: f.actor.id(),
        expected_epoch: 1,
        action: Action::ObserveRun {
            task_id: f.task,
            run_id: binding.run_id,
            lease_id: binding.lease_id,
            observation: event.observation.clone(),
        },
    };
    let connection = rusqlite::Connection::open(f.temp.path().join("state/baton.sqlite3")).unwrap();
    connection
        .execute_batch("DROP TRIGGER events_no_update")
        .unwrap();
    connection.execute("UPDATE events SET record=json_set(record,'$.evidence_hash',?1) WHERE task_id=?2 AND sequence=?3",
        rusqlite::params!["0".repeat(64),f.task.to_string(),i64::try_from(event.task_sequence).unwrap()]).unwrap();
    assert_eq!(
        f.store
            .run(&f.actor, f.task, binding.run_id)
            .unwrap_err()
            .code,
        ErrorCode::CorruptStore
    );
    assert_eq!(
        f.store.execute(&f.actor, &request).unwrap_err().code,
        ErrorCode::CorruptStore
    );
    drop(source);
    #[cfg(windows)]
    f.assert_child_stopped().await;
}

// M5-E05; a disconnected/cancelled stream is a durable gap, not a fresh resume cursor.
#[tokio::test]
async fn cancelled_or_timed_out_capture_records_gap_and_loss() {
    for cancelled in [false, true] {
        let mut f = Fixture::new();
        let session = f.launch("good");
        let binding = session.binding().clone();
        let source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(5))
            .await
            .unwrap();
        if cancelled {
            assert!(tokio::time::timeout(
                Duration::from_millis(200),
                source.capture_next(
                    &mut f.store,
                    &f.actor,
                    &adapter(),
                    "thread-1",
                    Duration::from_secs(10)
                )
            )
            .await
            .is_err());
        } else {
            assert_eq!(
                source
                    .capture_next(
                        &mut f.store,
                        &f.actor,
                        &adapter(),
                        "thread-1",
                        Duration::from_millis(200)
                    )
                    .await
                    .err()
                    .unwrap()
                    .code,
                ErrorCode::AdapterTimeout
            );
        }
        let run = f.store.run(&f.actor, f.task, binding.run_id).unwrap();
        assert!(run.event_gap);
        assert_eq!(run.state, RunState::RecoveryRequired);
        let events = f
            .store
            .execution_events(&f.actor, f.task, binding.run_id)
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event.observation, RunObservation::AdapterGap { .. })));
        #[cfg(windows)]
        f.assert_child_stopped().await;
    }
}

// M5-C05; FR-03/15: opt-in real vendor handshake, no model turn or user configuration.
#[tokio::test]
#[ignore = "requires explicit BATON_CODEX_BINARY and a separately checked vendor version"]
async fn installed_codex_initializes_in_isolated_home_without_model_turn() {
    let binary =
        std::env::var_os("BATON_CODEX_BINARY").expect("set explicit native Codex executable");
    let mut f = Fixture::new();
    let home = f.temp.path().join("state/codex-home");
    std::fs::create_dir(&home).unwrap();
    let mut environment = vec![
        ("CODEX_HOME".into(), home.as_os_str().to_owned()),
        ("HOME".into(), home.as_os_str().to_owned()),
        ("USERPROFILE".into(), home.as_os_str().to_owned()),
    ];
    if cfg!(windows) {
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            environment.push(("SystemRoot".into(), system_root));
        }
    }
    let spec = LaunchSpec::new(
        Path::new(&binary),
        &f.repo.path(),
        [
            "app-server".into(),
            "--listen".into(),
            "stdio://".into(),
            "-c".into(),
            "analytics.enabled=false".into(),
        ],
    )
    .unwrap()
    .with_piped_stdio()
    .with_environment(environment)
    .unwrap();
    let session = f.launch_spec(spec);
    let source = Source::connect(session, &mut f.store, &f.actor, Duration::from_secs(20))
        .await
        .unwrap();
    assert_eq!(source.runtime().platform_os, std::env::consts::OS);
    assert_eq!(
        f.store.task(&f.actor, f.task).unwrap().state,
        TaskState::Reserved
    );
    source
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    assert!(
        !home.join("sessions").exists(),
        "a model session was unexpectedly created"
    );
}
