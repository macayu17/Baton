use baton_checkpoint::Vault;
use baton_protocol::*;
use baton_test_support::GitFixture;

fn binding() -> SnapshotBinding {
    SnapshotBinding {
        checkpoint_id: Id::new_v4(),
        parent_id: None,
        repository_id: Id::new_v4(),
        task_id: Id::new_v4(),
        actor_id: Id::new_v4(),
        operation_id: Id::new_v4(),
        epoch: 1,
        event_cursor: 2,
    }
}
fn base(repo: &GitFixture) {
    for name in ["tracked.txt", "deleted.txt", "staged-deleted.txt"] {
        std::fs::write(repo.path().join(name), b"base\n").unwrap();
    }
    repo.git(&["add", "."]).unwrap();
    repo.git(&["commit", "-m", "fixture"]).unwrap();
}

// M4-SEQ-CAS; FR-05/09, INV-03/05: only a completed, bound receiver may become a source inside CAS.
#[test]
fn capture_completed_receiver_requires_receipt_binding_and_next_epoch() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&temp.path().join("vault")).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    let original = binding();
    let hash = vault.capture(&repo.path(), original.clone(), &[]).unwrap();
    let restore = Id::new_v4();
    let fp = "a".repeat(64);
    let report = vault
        .restore_recoverable(&repo.path(), &hash, restore, &fp)
        .unwrap();
    std::fs::write(
        std::path::Path::new(&report.worktree).join("tracked.txt"),
        b"receiver changed\n",
    )
    .unwrap();
    let mut next = original.clone();
    next.checkpoint_id = Id::new_v4();
    next.operation_id = Id::new_v4();
    next.parent_id = Some(original.checkpoint_id);
    next.actor_id = Id::new_v4();
    next.epoch = 2;
    assert_eq!(
        vault
            .capture(std::path::Path::new(&report.worktree), next.clone(), &[])
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    assert_eq!(
        vault
            .capture_restored(&report, &"b".repeat(64), next.clone(), &[])
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    let mut wrong = next.clone();
    wrong.task_id = Id::new_v4();
    assert_eq!(
        vault
            .capture_restored(&report, &fp, wrong, &[])
            .unwrap_err()
            .code,
        ErrorCode::IntegrityFailure
    );
    let mut wrong = next.clone();
    wrong.epoch = 1;
    assert_eq!(
        vault
            .capture_restored(&report, &fp, wrong, &[])
            .unwrap_err()
            .code,
        ErrorCode::StaleEpoch
    );
    let new_hash = vault.capture_restored(&report, &fp, next, &[]).unwrap();
    vault
        .verify_restored_source(&report, &fp, &new_hash)
        .unwrap();
    let manifest = vault.manifest(&new_hash).unwrap();
    let file = manifest
        .files
        .iter()
        .find(|file| file.path == "tracked.txt")
        .unwrap();
    assert_eq!(
        file.worktree.as_ref().unwrap().blob.hash,
        blake3::hash(b"receiver changed\n").to_hex().as_str()
    );
    assert_eq!(
        std::fs::read(repo.path().join("tracked.txt")).unwrap(),
        b"base\n"
    );
}

// M4-SEQ-CAS02; receipt format/identity failures cannot adopt an existing target.
#[test]
fn recapture_refuses_legacy_or_changed_git_identity_and_receiptless_targets() {
    for legacy in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let vault_path = temp.path().join("vault");
        let vault = Vault::create(&vault_path).unwrap();
        let repo = GitFixture::new().unwrap();
        base(&repo);
        let original = binding();
        let hash = vault.capture(&repo.path(), original.clone(), &[]).unwrap();
        let fp = "a".repeat(64);
        let report = vault
            .restore_recoverable(&repo.path(), &hash, Id::new_v4(), &fp)
            .unwrap();
        let path = vault_path
            .join("receipts")
            .join(blake3::hash(report.restore_id.as_bytes()).to_hex().as_str());
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if legacy {
            value.as_object_mut().unwrap().remove("format_version");
            value.as_object_mut().unwrap().remove("git_dir");
        } else {
            value["git_dir"] = repo.path().to_str().unwrap().into();
        }
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            vault
                .verify_receiver_identity(&report, &fp)
                .unwrap_err()
                .code,
            if legacy {
                ErrorCode::UnsupportedSchema
            } else {
                ErrorCode::IntegrityFailure
            }
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let without_receipt = vault.restore(&repo.path(), &hash, Id::new_v4()).unwrap();
        let mut next = original;
        next.epoch = 2;
        next.operation_id = Id::new_v4();
        next.checkpoint_id = Id::new_v4();
        assert_eq!(
            vault
                .capture_restored(&without_receipt, &fp, next, &[])
                .unwrap_err()
                .code,
            ErrorCode::IntegrityFailure
        );
    }
}

// M4-RS01; FR-09/13, INV-03/05/07: only a bound, completed restore may be reused.
#[test]
fn recoverable_restore_requires_receipt_binding_and_rechecks_receiver() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&temp.path().join("vault")).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    let hash = vault.capture(&repo.path(), binding(), &[]).unwrap();
    let id = Id::new_v4();
    let report = vault
        .restore_recoverable(&repo.path(), &hash, id, &"a".repeat(64))
        .unwrap();
    assert_eq!(
        report,
        vault
            .restore_recoverable(&repo.path(), &hash, id, &"a".repeat(64))
            .unwrap()
    );
    assert_eq!(
        vault
            .restore_recoverable(&repo.path(), &hash, id, &"b".repeat(64))
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    let file = std::path::Path::new(&report.worktree).join("tracked.txt");
    std::fs::write(&file, b"preserve receiver edit").unwrap();
    assert_eq!(
        vault
            .restore_recoverable(&repo.path(), &hash, id, &"a".repeat(64))
            .unwrap_err()
            .code,
        ErrorCode::IntegrityFailure
    );
    assert_eq!(std::fs::read(file).unwrap(), b"preserve receiver edit");
    let unbound = Id::new_v4();
    vault.restore(&repo.path(), &hash, unbound).unwrap();
    assert_eq!(
        vault
            .restore_recoverable(&repo.path(), &hash, unbound, &"a".repeat(64))
            .unwrap_err()
            .code,
        ErrorCode::DestinationExists
    );
}

// T-004/T-005-M2; FR-05/09, INV-03/05: distinct index/worktree bytes must survive.
#[test]
fn t004_t005_m2_roundtrip_preserves_index_worktree_and_source() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&temp.path().join("vault")).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    std::fs::write(repo.path().join("tracked.txt"), b"staged\n").unwrap();
    repo.git(&["add", "tracked.txt"]).unwrap();
    std::fs::write(repo.path().join("tracked.txt"), b"unstaged\r\n").unwrap();
    std::fs::remove_file(repo.path().join("deleted.txt")).unwrap();
    repo.git(&["rm", "staged-deleted.txt"]).unwrap();
    std::fs::write(repo.path().join("new λ.bin"), [0, 255, 13, 10]).unwrap();
    std::fs::write(repo.path().join("empty"), b"").unwrap();
    let index = std::fs::read(repo.path().join(".git/index")).unwrap();
    let start = std::time::Instant::now();
    let hash = vault
        .capture(
            &repo.path(),
            binding(),
            &["new λ.bin".into(), "empty".into()],
        )
        .unwrap();
    let capture_time = start.elapsed();
    let manifest = vault.manifest(&hash).unwrap();
    let start = std::time::Instant::now();
    let report = vault.restore(&repo.path(), &hash, Id::new_v4()).unwrap();
    eprintln!(
        "M2 fixture capture={capture_time:?}, restore={:?}",
        start.elapsed()
    );
    let target = std::path::Path::new(&report.worktree);
    assert_eq!(
        std::fs::read(target.join("tracked.txt")).unwrap(),
        b"unstaged\r\n"
    );
    assert_eq!(
        std::fs::read(target.join("new λ.bin")).unwrap(),
        [0, 255, 13, 10]
    );
    assert_eq!(std::fs::read(target.join("empty")).unwrap(), b"");
    assert!(!target.join("deleted.txt").exists());
    assert!(!target.join("staged-deleted.txt").exists());
    let bytes = std::process::Command::new("git")
        .current_dir(target)
        .args(["show", ":tracked.txt"])
        .output()
        .unwrap();
    assert!(bytes.status.success());
    assert_eq!(bytes.stdout, b"staged\n");
    assert_eq!(
        std::fs::read(repo.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(
        std::fs::read(repo.path().join("tracked.txt")).unwrap(),
        b"unstaged\r\n"
    );
    assert_eq!(report.quality, RecoveryQuality::Blocked);
    assert!(report
        .reasons
        .contains(&CheckpointReason::SupervisionUnavailable));
    assert_eq!(manifest.binding.epoch, 1);
    vault.verify_restore(&repo.path(), &hash, &report).unwrap();
}

// T-004-M2; FR-05/09/16, INV-03: secrets/ignored omissions must be explicit.
#[test]
fn m2_002_sensitive_and_ignored_bytes_are_never_materialized() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&temp.path().join("vault")).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    std::fs::write(repo.path().join(".env"), b"PASSWORD=synthetic-secret-value").unwrap();
    std::fs::write(repo.path().join(".gitignore"), b"ignored.txt\n").unwrap();
    repo.git(&["add", ".env", ".gitignore"]).unwrap();
    repo.git(&["commit", "-m", "secret fixture"]).unwrap();
    std::fs::write(repo.path().join("ignored.txt"), b"do not capture").unwrap();
    std::fs::write(repo.path().join("unselected.txt"), b"not opted in").unwrap();
    std::fs::write(
        repo.path().join("token.txt"),
        b"ghp_abcdefghijklmnopqrstuvwxyz0123456789",
    )
    .unwrap();
    let hash = vault
        .capture(&repo.path(), binding(), &["token.txt".into()])
        .unwrap();
    let manifest = vault.manifest(&hash).unwrap();
    assert!(manifest
        .gaps
        .iter()
        .any(|g| g.path == ".env" && g.reason == GapReason::Sensitive));
    assert!(manifest
        .gaps
        .iter()
        .any(|g| g.path == "ignored.txt" && g.reason == GapReason::Ignored));
    assert!(manifest
        .gaps
        .iter()
        .any(|g| g.path == "unselected.txt" && g.reason == GapReason::NotIncluded));
    assert!(manifest
        .gaps
        .iter()
        .any(|g| g.path == "token.txt" && g.reason == GapReason::Sensitive));
    let report = vault.restore(&repo.path(), &hash, Id::new_v4()).unwrap();
    for name in [".env", "ignored.txt", "unselected.txt", "token.txt"] {
        assert!(!std::path::Path::new(&report.worktree).join(name).exists());
    }
}

// T-005-M2; FR-09, INV-05: existing paths remain untouched even for retries.
#[test]
fn t005_m2_existing_receiver_is_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&temp.path().join("vault")).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    let hash = vault.capture(&repo.path(), binding(), &[]).unwrap();
    let restore = Id::new_v4();
    let first = vault.restore(&repo.path(), &hash, restore).unwrap();
    std::fs::write(
        std::path::Path::new(&first.worktree).join("tracked.txt"),
        b"receiver edits",
    )
    .unwrap();
    assert_eq!(
        vault
            .restore(&repo.path(), &hash, restore)
            .unwrap_err()
            .code,
        ErrorCode::DestinationExists
    );
    assert_eq!(
        std::fs::read(std::path::Path::new(&first.worktree).join("tracked.txt")).unwrap(),
        b"receiver edits"
    );
    assert!(vault.verify_restore(&repo.path(), &hash, &first).is_err());
}

// T-004-M2; FR-05/09, INV-03: a corrupt/missing required blob must block restore.
#[test]
fn t004_m2_corrupt_artifact_blocks_before_receiver_creation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("vault");
    let vault = Vault::create(&root).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    let hash = vault.capture(&repo.path(), binding(), &[]).unwrap();
    let manifest = vault.manifest(&hash).unwrap();
    let object = manifest.files[0].index.as_ref().unwrap();
    std::fs::write(root.join("blobs").join(&object.blob.hash), b"corrupt").unwrap();
    let id = Id::new_v4();
    assert_eq!(
        vault.restore(&repo.path(), &hash, id).unwrap_err().code,
        ErrorCode::IntegrityFailure
    );
    assert!(!root.join("restores").join(id.to_string()).exists());
}

// M2-MATRIX; FR-05/09, INV-03/05: unsupported Git state never becomes a checkpoint.
#[test]
fn m2_unsupported_git_states_fail_closed() {
    for kind in ["intent", "assume", "split", "sparse", "locked"] {
        let temp = tempfile::tempdir().unwrap();
        let vault = Vault::create(&temp.path().join("vault")).unwrap();
        let repo = GitFixture::new().unwrap();
        base(&repo);
        match kind {
            "intent" => {
                std::fs::write(repo.path().join("intent"), b"new").unwrap();
                repo.git(&["add", "-N", "intent"]).unwrap();
            }
            "assume" => {
                repo.git(&["update-index", "--assume-unchanged", "tracked.txt"])
                    .unwrap();
            }
            "split" => {
                repo.git(&["update-index", "--split-index"]).unwrap();
            }
            "sparse" => {
                repo.git(&["config", "core.sparseCheckout", "true"])
                    .unwrap();
            }
            _ => {
                std::fs::write(repo.path().join(".git/index.lock"), b"busy").unwrap();
            }
        }
        assert_eq!(
            vault
                .capture(&repo.path(), binding(), &[])
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedRepository,
            "{kind}"
        );
    }
}

// M2-MATRIX; FR-05/16, INV-03: LFS/submodules are gaps, never implicit fetches.
#[test]
fn m2_lfs_and_submodules_are_explicit_gaps() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&temp.path().join("vault")).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    std::fs::write(
        repo.path().join("pointer"),
        b"version https://git-lfs.github.com/spec/v1\noid sha256:dummy\nsize 999\n",
    )
    .unwrap();
    repo.git(&["add", "pointer"]).unwrap();
    let head = String::from_utf8(repo.git(&["rev-parse", "HEAD"]).unwrap()).unwrap();
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{},submodule", head.trim()),
    ])
    .unwrap();
    let hash = vault.capture(&repo.path(), binding(), &[]).unwrap();
    let manifest = vault.manifest(&hash).unwrap();
    assert!(manifest
        .gaps
        .iter()
        .any(|g| g.path == "pointer" && g.reason == GapReason::LfsPointer));
    assert!(manifest
        .gaps
        .iter()
        .any(|g| g.path == "submodule" && g.reason == GapReason::Submodule));
}

// M2-MATRIX; FR-05/09/16, INV-05: a hardlink outside the source is never copied.
#[test]
fn m2_source_hardlinks_are_excluded_on_native_platform() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&temp.path().join("vault")).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    let outside = temp.path().join("outside");
    std::fs::write(&outside, b"outside private data").unwrap();
    std::fs::hard_link(&outside, repo.path().join("linked.txt")).unwrap();
    let hash = vault
        .capture(&repo.path(), binding(), &["linked.txt".into()])
        .unwrap();
    assert!(vault
        .manifest(&hash)
        .unwrap()
        .gaps
        .iter()
        .any(|g| g.path == "linked.txt" && g.reason == GapReason::UnsupportedLink));
    let secret_hash = blake3::hash(b"outside private data").to_hex().to_string();
    assert!(!temp.path().join("vault/blobs").join(secret_hash).exists());
}

// M2-MATRIX; FR-05/09, INV-05: staged renames and executable modes survive.
#[test]
fn m2_rename_and_executable_mode_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&temp.path().join("vault")).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    repo.git(&["mv", "tracked.txt", "renamed.txt"]).unwrap();
    repo.git(&["update-index", "--chmod=+x", "renamed.txt"])
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            repo.path().join("renamed.txt"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let hash = vault.capture(&repo.path(), binding(), &[]).unwrap();
    let manifest = vault.manifest(&hash).unwrap();
    assert_eq!(
        manifest
            .files
            .iter()
            .find(|f| f.path == "renamed.txt")
            .unwrap()
            .index
            .as_ref()
            .unwrap()
            .mode,
        FileMode::Executable
    );
    let report = vault.restore(&repo.path(), &hash, Id::new_v4()).unwrap();
    assert!(!std::path::Path::new(&report.worktree)
        .join("tracked.txt")
        .exists());
    assert_eq!(
        std::fs::read(std::path::Path::new(&report.worktree).join("renamed.txt")).unwrap(),
        b"base\n"
    );
    vault.verify_restore(&repo.path(), &hash, &report).unwrap();
    #[cfg(windows)]
    assert!(manifest
        .gaps
        .iter()
        .any(|g| g.reason == GapReason::ExecutableMetadata));
}

// M2-MATRIX; FR-05/09/16, INV-05: do not follow hostile source symlinks.
#[cfg(unix)]
#[test]
fn m2_source_symlinks_never_read_outside_root() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&temp.path().join("vault")).unwrap();
    let repo = GitFixture::new().unwrap();
    base(&repo);
    let outside = temp.path().join("outside");
    std::fs::write(&outside, b"outside private data").unwrap();
    std::os::unix::fs::symlink(&outside, repo.path().join("linked.txt")).unwrap();
    let hash = vault
        .capture(&repo.path(), binding(), &["linked.txt".into()])
        .unwrap();
    assert!(vault
        .manifest(&hash)
        .unwrap()
        .gaps
        .iter()
        .any(|g| g.path == "linked.txt" && g.reason == GapReason::UnsupportedLink));
}
