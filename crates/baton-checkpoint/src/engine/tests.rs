use super::*;
use baton_test_support::GitFixture;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

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
fn fixture() -> GitFixture {
    let repo = GitFixture::new().unwrap();
    std::fs::write(repo.path().join("file.txt"), b"before\n").unwrap();
    repo.git(&["add", "file.txt"]).unwrap();
    repo.git(&["commit", "-m", "fixture"]).unwrap();
    repo
}
fn child(
    root: &Path,
    repo: &Path,
    ready: &Path,
    point: &str,
    binding: &SnapshotBinding,
    release: Option<&Path>,
) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "engine::tests::cas_child",
            "--ignored",
            "--nocapture",
        ])
        .env("BATON_CAS_TEST_ROOT", root)
        .env("BATON_CAS_TEST_SOURCE", repo)
        .env("BATON_CAS_TEST_READY", ready)
        .env("BATON_CAS_TEST_POINT", point)
        .env(
            "BATON_CAS_TEST_BINDING",
            serde_json::to_string(binding).unwrap(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(release) = release {
        command.env("BATON_CAS_TEST_RELEASE", release);
    }
    command
}
fn wait_ready(child: &mut std::process::Child, ready: &Path) {
    let start = Instant::now();
    while !ready.exists() && start.elapsed() < Duration::from_secs(30) {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("CAS child exited early: {status}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !ready.exists() {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("CAS boundary not reached");
    }
}

// T-004-M2; FR-05, INV-03: exact CAS boundaries never expose incomplete roots.
#[test]
fn t004_m2_hard_kills_across_cas_publication() {
    for point in [
        "after_staging_manifest",
        "before_object_write",
        "after_blob_flush",
        "after_blob_link",
        "before_manifest_write",
        "after_manifest_flush",
        "after_manifest_link",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("vault");
        let vault = Vault::create(&root).unwrap();
        let repo = fixture();
        let ready = temp.path().join("ready");
        let mut process = child(&root, &repo.path(), &ready, point, &binding(), None)
            .spawn()
            .unwrap();
        wait_ready(&mut process, &ready);
        process.kill().unwrap();
        process.wait().unwrap();
        let roots: Vec<_> = fs::read_dir(root.join("manifests"))
            .unwrap()
            .map(|r| r.unwrap().file_name().to_str().unwrap().to_owned())
            .collect();
        assert_eq!(roots.len(), usize::from(point == "after_manifest_link"));
        for hash in roots {
            vault.manifest(&hash).unwrap();
        }
        assert_eq!(fs::read(repo.path().join("file.txt")).unwrap(), b"before\n");
    }
}

// M2-CONSISTENCY; FR-05, INV-09: stable-looking metadata cannot hide changed bytes.
#[test]
fn m2_source_change_between_scans_blocks_publication() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("vault");
    Vault::create(&root).unwrap();
    let repo = fixture();
    let ready = temp.path().join("ready");
    let release = temp.path().join("release");
    let mut process = child(
        &root,
        &repo.path(),
        &ready,
        "between_scans",
        &binding(),
        Some(&release),
    )
    .spawn()
    .unwrap();
    wait_ready(&mut process, &ready);
    fs::write(repo.path().join("file.txt"), b"changed\n").unwrap();
    fs::write(&release, b"continue").unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = process.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(30) {
            process.kill().unwrap();
            process.wait().unwrap();
            panic!("CAS child stalled");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read_dir(root.join("manifests")).unwrap().count(), 0);
}

// M2-CAS; FR-05, INV-03: bounded decoding/dedup and staged/live roots guide GC.
#[test]
fn m2_compression_dedup_and_reference_aware_collection() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("vault");
    let vault = Vault::create(&root).unwrap();
    let repo = fixture();
    fs::write(repo.path().join("file.txt"), vec![b'x'; 64 * 1024]).unwrap();
    repo.git(&["add", "file.txt"]).unwrap();
    let hash = vault.capture(&repo.path(), binding(), &[]).unwrap();
    let manifest = vault.manifest(&hash).unwrap();
    assert_eq!(
        manifest.files[0].index.as_ref().unwrap().blob.encoding,
        BlobEncoding::Zstd
    );
    assert_eq!(fs::read_dir(root.join("blobs")).unwrap().count(), 1);
    let orphan = vault.put(b"unreferenced").unwrap();
    assert_eq!(
        vault.collect_orphans(std::slice::from_ref(&hash)).unwrap(),
        1
    );
    assert!(!root.join("blobs").join(orphan.hash).exists());
    vault.manifest(&hash).unwrap();
    let mut hostile = manifest.clone();
    hostile.files[0].index.as_mut().unwrap().blob.bytes = 1;
    let json = serde_json::to_vec(&hostile).unwrap();
    let hostile_hash = digest(&json);
    create_file(&root.join("manifests").join(&hostile_hash), &json).unwrap();
    assert_eq!(
        vault.manifest(&hostile_hash).unwrap_err().code,
        ErrorCode::IntegrityFailure
    );
    assert_eq!(
        vault.collect_orphans(&[hash]).unwrap_err().code,
        ErrorCode::IntegrityFailure
    );
}

// M2-PATH; FR-09, INV-05: valid hashes cannot authorize traversal or collisions.
#[test]
fn m2_crafted_manifest_paths_fail_before_materialization() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("vault");
    let vault = Vault::create(&root).unwrap();
    let repo = fixture();
    let hash = vault.capture(&repo.path(), binding(), &[]).unwrap();
    let original = vault.manifest(&hash).unwrap();
    for path in [
        "../escape",
        "/absolute",
        ".git/config",
        "GIT~1/config",
        "C:/escape",
        "nul.txt",
        "trailing.",
        "a\\b",
    ] {
        let mut hostile = original.clone();
        hostile.files[0].path = path.into();
        let json = serde_json::to_vec(&hostile).unwrap();
        let hash = digest(&json);
        create_file(&root.join("manifests").join(&hash), &json).unwrap();
        assert_eq!(
            vault
                .restore(&repo.path(), &hash, Id::new_v4())
                .unwrap_err()
                .code,
            ErrorCode::UnsafePath
        );
    }
    let mut hostile = original;
    let mut duplicate = hostile.files[0].clone();
    duplicate.path = "FILE.TXT".into();
    hostile.files.push(duplicate);
    let json = serde_json::to_vec(&hostile).unwrap();
    let hash = digest(&json);
    create_file(&root.join("manifests").join(&hash), &json).unwrap();
    assert_eq!(
        vault.manifest(&hash).unwrap_err().code,
        ErrorCode::UnsafePath
    );
    assert_eq!(fs::read_dir(root.join("restores")).unwrap().count(), 0);
}

// T-005-M2; FR-09, INV-05: killed restoration preserves source and partial receiver.
#[test]
fn t005_m2_interrupted_receiver_is_preserved_and_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("vault");
    let vault = Vault::create(&root).unwrap();
    let repo = fixture();
    let bind = binding();
    let hash = vault.capture(&repo.path(), bind.clone(), &[]).unwrap();
    let restore = Id::new_v4();
    let ready = temp.path().join("ready");
    let index = fs::read(repo.path().join(".git/index")).unwrap();
    let mut process = child(
        &root,
        &repo.path(),
        &ready,
        "after_worktree_add",
        &bind,
        None,
    )
    .env("BATON_CAS_TEST_RESTORE_HASH", &hash)
    .env("BATON_CAS_TEST_RESTORE_ID", restore.to_string())
    .spawn()
    .unwrap();
    wait_ready(&mut process, &ready);
    process.kill().unwrap();
    process.wait().unwrap();
    let target = root.join("restores").join(restore.to_string());
    assert!(target.join(".git").is_file());
    assert!(!target.join("file.txt").exists());
    assert_eq!(
        vault
            .restore(&repo.path(), &hash, restore)
            .unwrap_err()
            .code,
        ErrorCode::DestinationExists
    );
    assert_eq!(fs::read(repo.path().join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(repo.path().join("file.txt")).unwrap(), b"before\n");
}

// T-004-M2 subprocess entry; invoked at seven write boundaries and one changed scan.
// M4-RS03; FR-09, INV-03/05: partial targets without receipts are never adopted.
#[test]
fn m4_restore_receipt_crash_boundaries_preserve_partial_targets() {
    for point in [
        "after_worktree_add",
        "before_restore_receipt",
        "after_restore_receipt",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("vault");
        let vault = Vault::create(&root).unwrap();
        let repo = fixture();
        let binding = binding();
        let hash = vault.capture(&repo.path(), binding.clone(), &[]).unwrap();
        let restore_id = Id::new_v4();
        let ready = temp.path().join("ready");
        let mut process = child(&root, &repo.path(), &ready, point, &binding, None)
            .env("BATON_CAS_TEST_RESTORE_HASH", &hash)
            .env("BATON_CAS_TEST_RESTORE_ID", restore_id.to_string())
            .env("BATON_CAS_TEST_RESTORE_FINGERPRINT", "a".repeat(64))
            .spawn()
            .unwrap();
        wait_ready(&mut process, &ready);
        process.kill().unwrap();
        process.wait().unwrap();
        let target = root.join("restores").join(restore_id.to_string());
        assert!(target.exists());
        if point == "after_restore_receipt" {
            let report = vault
                .restore_recoverable(&repo.path(), &hash, restore_id, &"a".repeat(64))
                .unwrap();
            assert_eq!(report.quality, RecoveryQuality::Blocked);
        } else {
            fs::write(target.join("preserve"), b"partial receiver").unwrap();
            assert_eq!(
                vault
                    .restore_recoverable(&repo.path(), &hash, restore_id, &"a".repeat(64))
                    .unwrap_err()
                    .code,
                ErrorCode::DestinationExists
            );
            assert_eq!(
                fs::read(target.join("preserve")).unwrap(),
                b"partial receiver"
            );
        }
    }
}

#[test]
#[ignore = "subprocess entry explicitly invoked by M2 fault tests"]
fn cas_child() {
    let root = std::env::var_os("BATON_CAS_TEST_ROOT").unwrap();
    let source = std::env::var_os("BATON_CAS_TEST_SOURCE").unwrap();
    let binding = serde_json::from_str(&std::env::var("BATON_CAS_TEST_BINDING").unwrap()).unwrap();
    let vault = Vault::open(Path::new(&root)).unwrap();
    if let Ok(hash) = std::env::var("BATON_CAS_TEST_RESTORE_HASH") {
        let id = Id::parse_str(&std::env::var("BATON_CAS_TEST_RESTORE_ID").unwrap()).unwrap();
        if let Ok(fingerprint) = std::env::var("BATON_CAS_TEST_RESTORE_FINGERPRINT") {
            vault
                .restore_recoverable(Path::new(&source), &hash, id, &fingerprint)
                .unwrap();
        } else {
            vault.restore(Path::new(&source), &hash, id).unwrap();
        }
        panic!("restore boundary not reached");
    }
    let result = vault.capture(Path::new(&source), binding, &[]);
    if std::env::var_os("BATON_CAS_TEST_RELEASE").is_some() {
        assert_eq!(result.unwrap_err().code, ErrorCode::SourceChanged);
    } else {
        result.unwrap();
        panic!("fault boundary not reached");
    }
}
