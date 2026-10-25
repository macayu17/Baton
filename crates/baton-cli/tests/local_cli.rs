use baton_protocol::*;
use baton_test_support::GitFixture;
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn command(root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_baton"));
    c.arg("--state").arg(root).arg("--json");
    c
}
fn call(root: &Path, args: &[&str]) -> LocalResponse {
    let output = command(root).args(args).output().unwrap();
    let response: LocalResponse = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "invalid CLI response: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(output.status.success(), response.result.is_ok());
    response
}
struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn daemon(root: &Path) -> Daemon {
    let child = command(root)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut daemon = Daemon(child);
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "daemon exited before ready"
        );
        if call(root, &["status"]).result.is_ok() {
            return daemon;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("daemon did not become ready");
}

// M4-CLI01; FR-01/02/04/12/13, INV-05/07: real CLI and daemon survive restart.
#[test]
fn local_cli_initializes_preserves_source_and_retries_after_daemon_crash() {
    let temp = tempfile::tempdir().unwrap();
    let root: PathBuf = temp.path().join("state");
    let repo = GitFixture::new().unwrap();
    std::fs::write(repo.path().join("sentinel"), b"preserve user content").unwrap();
    let before = repo
        .git(&["status", "--porcelain=v1", "--untracked-files=all"])
        .unwrap();
    let init = call(&root, &["init", "--repo", repo.path().to_str().unwrap()]);
    let LocalReply::Mutation(Reply::Repository(repository)) = init.result.unwrap() else {
        panic!()
    };
    let init_again = call(&root, &["init", "--repo", repo.path().to_str().unwrap()]);
    assert_eq!(
        init_again.result.unwrap(),
        LocalReply::Mutation(Reply::Repository(repository))
    );
    let owner = daemon(&root);
    let operation = Id::new_v4().to_string();
    let task = operation.clone();
    let repository = repository.to_string();
    let arguments = [
        "--operation-id",
        &operation,
        "create-task",
        "--repository",
        &repository,
        "--goal",
        "synthetic unfinished work",
    ];
    let created = call(&root, &arguments);
    assert!(created.result.is_ok());
    assert_eq!(created.continuation_quality, RecoveryQuality::Blocked);
    drop(owner); // actual process kill; no shutdown acknowledgement
    let _owner = daemon(&root);
    assert_eq!(call(&root, &arguments), created);
    let events = call(&root, &["events", "--task", &task]);
    let LocalReply::Events(events) = events.result.unwrap() else {
        panic!()
    };
    assert_eq!(events.len(), 1);
    assert_eq!(
        std::fs::read(repo.path().join("sentinel")).unwrap(),
        b"preserve user content"
    );
    assert_eq!(
        repo.git(&["status", "--porcelain=v1", "--untracked-files=all"])
            .unwrap(),
        before
    );
}

// M4-CLI05; T-008/T-009 subcases: transfer intent survives coordinator hard kill.
#[test]
fn local_cli_transfer_intent_preflight_restart_and_abort() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let repo = GitFixture::new().unwrap();
    repo.git(&["commit", "--allow-empty", "-m", "fixture"])
        .unwrap();
    let LocalReply::Mutation(Reply::Repository(repository)) =
        call(&root, &["init", "--repo", repo.path().to_str().unwrap()])
            .result
            .unwrap()
    else {
        panic!()
    };
    let owner = daemon(&root);
    let task = Id::new_v4().to_string();
    call(
        &root,
        &[
            "--operation-id",
            &task,
            "create-task",
            "--repository",
            &repository.to_string(),
            "--goal",
            "synthetic transfer",
        ],
    )
    .result
    .unwrap();
    let LocalReply::Mutation(Reply::Lease(lease)) =
        call(&root, &["claim", "--task", &task]).result.unwrap()
    else {
        panic!()
    };
    let lease_id = lease.lease_id.to_string();
    let receiver = lease.owner_id.to_string();
    let transfer = Id::new_v4().to_string();
    let args = [
        "--epoch",
        "1",
        "--operation-id",
        &transfer,
        "transfer-request",
        "--task",
        &task,
        "--lease",
        &lease_id,
        "--receiver",
        &receiver,
    ];
    let requested = call(&root, &args);
    requested.result.as_ref().unwrap();
    drop(owner);
    let _owner = daemon(&root);
    assert_eq!(call(&root, &args), requested);
    let checkpoint = Id::new_v4().to_string();
    call(
        &root,
        &[
            "--epoch",
            "1",
            "--operation-id",
            &checkpoint,
            "checkpoint",
            "--task",
            &task,
            "--lease",
            &lease_id,
        ],
    )
    .result
    .unwrap();
    let preflight = call(
        &root,
        &[
            "--epoch",
            "1",
            "transfer-preflight",
            "--task",
            &task,
            "--lease",
            &lease_id,
            "--transfer",
            &transfer,
            "--checkpoint",
            &checkpoint,
        ],
    );
    let LocalReply::Mutation(Reply::Transfer(blocked)) = preflight.result.unwrap() else {
        panic!()
    };
    assert_eq!(blocked.state, TransferState::Blocked);
    assert!(blocked
        .reasons
        .contains(&TransferReason::SourceFenceUnavailable));
    assert_eq!(blocked.committed_epoch, None);
    let info = call(
        &root,
        &["transfer-info", "--task", &task, "--transfer", &transfer],
    );
    assert_eq!(info.result.unwrap(), LocalReply::Transfer(blocked));
    let stage_id = Id::new_v4().to_string();
    let restore_id = Id::new_v4().to_string();
    assert_eq!(
        call(
            &root,
            &[
                "--epoch",
                "1",
                "transfer-begin-staging",
                "--task",
                &task,
                "--transfer",
                &transfer,
                "--restore-id",
                &restore_id,
                "--stage-operation-id",
                &stage_id
            ]
        )
        .result
        .unwrap_err()
        .code,
        ErrorCode::Blocked
    );
    assert_eq!(
        call(
            &root,
            &[
                "--epoch",
                "1",
                "transfer-commit",
                "--task",
                &task,
                "--transfer",
                &transfer
            ]
        )
        .result
        .unwrap_err()
        .code,
        ErrorCode::Blocked
    );
    assert_eq!(
        call(
            &root,
            &[
                "--epoch",
                "1",
                "--operation-id",
                &stage_id,
                "transfer-stage",
                "--task",
                &task,
                "--transfer",
                &transfer
            ]
        )
        .result
        .unwrap_err()
        .code,
        ErrorCode::Blocked
    );
    assert_eq!(
        call(
            &root,
            &[
                "--epoch",
                "1",
                "transfer-activate",
                "--task",
                &task,
                "--transfer",
                &transfer
            ]
        )
        .result
        .unwrap_err()
        .code,
        ErrorCode::ExecutionNotEnabled
    );
    assert!(!root.join("checkpoints/restores").join(&restore_id).exists());
    call(
        &root,
        &[
            "--epoch",
            "1",
            "transfer-recover",
            "--task",
            &task,
            "--transfer",
            &transfer,
        ],
    )
    .result
    .unwrap();
    let abort = call(
        &root,
        &[
            "--epoch",
            "1",
            "transfer-abort",
            "--task",
            &task,
            "--transfer",
            &transfer,
        ],
    );
    let LocalReply::Mutation(Reply::Transfer(aborted)) = abort.result.unwrap() else {
        panic!()
    };
    assert_eq!(aborted.state, TransferState::FailedPrecommit);
    let LocalReply::Task(task) = call(&root, &["status", "--task", &task]).result.unwrap() else {
        panic!()
    };
    assert_eq!(task.epoch, 1);
    assert_eq!(task.owner_id, Some(lease.owner_id));
}

// M4-CLI02; FR-05/09/12/13, INV-03/05/07: real capture/restore stays forensic.
#[test]
fn local_cli_checkpoint_restore_and_cache_validation_preserve_work() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let repo = GitFixture::new().unwrap();
    std::fs::write(repo.path().join("file.txt"), b"base\n").unwrap();
    repo.git(&["add", "file.txt"]).unwrap();
    repo.git(&["commit", "-m", "fixture"]).unwrap();
    std::fs::write(repo.path().join("file.txt"), b"staged\n").unwrap();
    repo.git(&["add", "file.txt"]).unwrap();
    std::fs::write(repo.path().join("file.txt"), b"working\r\n").unwrap();
    std::fs::write(repo.path().join("new.bin"), [0, 255, 7]).unwrap();
    let before = std::fs::read(repo.path().join(".git/index")).unwrap();
    let LocalReply::Mutation(Reply::Repository(repository)) =
        call(&root, &["init", "--repo", repo.path().to_str().unwrap()])
            .result
            .unwrap()
    else {
        panic!()
    };
    let _owner = daemon(&root);
    let repository = repository.to_string();
    let task = Id::new_v4().to_string();
    call(
        &root,
        &[
            "--operation-id",
            &task,
            "create-task",
            "--repository",
            &repository,
            "--goal",
            "synthetic snapshot",
        ],
    )
    .result
    .unwrap();
    let LocalReply::Mutation(Reply::Lease(lease)) =
        call(&root, &["claim", "--task", &task]).result.unwrap()
    else {
        panic!()
    };
    let lease = lease.lease_id.to_string();
    let checkpoint = Id::new_v4().to_string();
    let captured = call(
        &root,
        &[
            "--operation-id",
            &checkpoint,
            "--epoch",
            "1",
            "checkpoint",
            "--task",
            &task,
            "--lease",
            &lease,
            "--include-untracked",
            "new.bin",
        ],
    );
    assert_eq!(captured.continuation_quality, RecoveryQuality::Blocked);
    let LocalReply::Mutation(Reply::Checkpoint(record)) = captured.result.unwrap() else {
        panic!()
    };
    assert_eq!(record.quality, RecoveryQuality::Blocked);
    let restore = Id::new_v4().to_string();
    let args = [
        "--operation-id",
        &restore,
        "--epoch",
        "1",
        "restore",
        "--task",
        &task,
        "--lease",
        &lease,
        "--checkpoint",
        &checkpoint,
    ];
    let restored = call(&root, &args);
    let LocalReply::Mutation(Reply::Restored(report)) = restored.result.clone().unwrap() else {
        panic!()
    };
    let receiver = Path::new(&report.worktree);
    assert_eq!(
        std::fs::read(receiver.join("file.txt")).unwrap(),
        b"working\r\n"
    );
    assert_eq!(
        std::fs::read(receiver.join("new.bin")).unwrap(),
        [0, 255, 7]
    );
    assert_eq!(call(&root, &args), restored);
    assert_eq!(
        std::fs::read(repo.path().join(".git/index")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(repo.path().join("file.txt")).unwrap(),
        b"working\r\n"
    );
    std::fs::write(receiver.join("file.txt"), b"preserve receiver edit").unwrap();
    assert_eq!(
        call(&root, &args).result.unwrap_err().code,
        ErrorCode::IntegrityFailure
    );
    assert_eq!(
        std::fs::read(receiver.join("file.txt")).unwrap(),
        b"preserve receiver edit"
    );
}

// M4-CLI03; FR-12/13, INV-02/09: forged supervisor facts are denied and audited.
#[tokio::test]
async fn daemon_rejects_and_audits_supervisor_observations_from_local_clients() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let repo = GitFixture::new().unwrap();
    call(&root, &["init", "--repo", repo.path().to_str().unwrap()])
        .result
        .unwrap();
    let _owner = daemon(&root);
    let request_id = Id::new_v4();
    let request = LocalRequest {
        schema_version: SchemaVersion::V0_1_0,
        request_id,
        expected_epoch: 1,
        command: LocalCommand::Invoke {
            action: Action::ObserveRun {
                task_id: Id::new_v4(),
                lease_id: Id::new_v4(),
                run_id: Id::new_v4(),
                observation: RunObservation::TreeStopped,
            },
        },
    };
    let result = baton_cli::request(root.clone(), request.clone())
        .await
        .unwrap();
    assert_eq!(
        result.result.as_ref().unwrap_err().code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        baton_cli::request(root.clone(), request).await.unwrap(),
        result
    );
    let c = rusqlite::Connection::open(root.join("baton.sqlite3")).unwrap();
    let reason: String = c.query_row("SELECT json_extract(record,'$.reason') FROM audit WHERE json_extract(record,'$.operation_id')=?1", [request_id.to_string()], |r| r.get(0)).unwrap();
    assert_eq!(reason, "FORBIDDEN");
    let events: i64 = c
        .query_row("SELECT count(*) FROM execution_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(events, 0);
}

// T-008-M4 local transport subcase; FR-07/08, INV-01/07: duplicate concurrent claims.
#[test]
fn concurrent_local_claim_retries_return_one_persisted_lease() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let repo = GitFixture::new().unwrap();
    let LocalReply::Mutation(Reply::Repository(repository)) =
        call(&root, &["init", "--repo", repo.path().to_str().unwrap()])
            .result
            .unwrap()
    else {
        panic!()
    };
    let _owner = daemon(&root);
    let repository = repository.to_string();
    let task = Id::new_v4().to_string();
    call(
        &root,
        &[
            "--operation-id",
            &task,
            "create-task",
            "--repository",
            &repository,
            "--goal",
            "synthetic contention",
        ],
    )
    .result
    .unwrap();
    let operation = Id::new_v4().to_string();
    let replies = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    call(
                        &root,
                        &["--operation-id", &operation, "claim", "--task", &task],
                    )
                })
            })
            .collect();
        jobs.into_iter()
            .map(|job| job.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(replies[0].result.is_ok(), "{replies:?}");
    assert!(replies.iter().all(|r| r == &replies[0]), "{replies:?}");
    let LocalReply::Events(events) = call(&root, &["events", "--task", &task]).result.unwrap()
    else {
        panic!()
    };
    assert_eq!(events.len(), 2);
}

// M4-CLI04; FR-13/19: human diagnostics explain unavailable opt-in coordination.
#[test]
fn human_status_explains_daemon_start_and_never_starts_it_implicitly() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let repo = GitFixture::new().unwrap();
    call(&root, &["init", "--repo", repo.path().to_str().unwrap()])
        .result
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_baton"))
        .arg("--state")
        .arg(&root)
        .arg("status")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("DAEMON_UNAVAILABLE") && text.contains("Start the local daemon explicitly")
    );
    assert!(text.contains("BLOCKED (EXECUTION_NOT_ENABLED)"));
}
