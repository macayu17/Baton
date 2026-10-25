use baton_protocol::ErrorCode;
use baton_test_support::GitFixture;

// M0-004; FR-01, FR-09, INV-05. Wrong index/worktree bytes must fail.
#[test]
fn m0_004_fixture_preserves_distinct_git_states_and_bytes() {
    let repo = GitFixture::new().unwrap();
    std::fs::write(repo.path().join("tracked.txt"), b"base\n").unwrap();
    repo.git(&["add", "tracked.txt"]).unwrap();
    repo.git(&["commit", "-m", "fixture"]).unwrap();
    std::fs::write(repo.path().join("tracked.txt"), b"staged\r\n").unwrap();
    repo.git(&["add", "tracked.txt"]).unwrap();
    std::fs::write(repo.path().join("tracked.txt"), b"unstaged\n").unwrap();
    std::fs::write(repo.path().join("untracked.bin"), [0, 255, 10, 13]).unwrap();
    assert_eq!(repo.git(&["show", "HEAD:tracked.txt"]).unwrap(), b"base\n");
    assert_eq!(repo.git(&["show", ":tracked.txt"]).unwrap(), b"staged\r\n");
    assert_eq!(
        std::fs::read(repo.path().join("tracked.txt")).unwrap(),
        b"unstaged\n"
    );
    assert_eq!(
        repo.git(&["status", "--porcelain=v1", "--untracked-files=all"])
            .unwrap(),
        b"MM tracked.txt\n?? untracked.bin\n"
    );
}

// M0-005; FR-01, FR-09, INV-05. Reusing fixture roots must fail.
#[test]
fn m0_005_fixtures_are_independent_and_handle_unicode_spaces() {
    let first = GitFixture::new().unwrap();
    let second = GitFixture::new().unwrap();
    assert_ne!(first.path(), second.path());
    assert!(first
        .path()
        .to_string_lossy()
        .contains("repo with spaces-λ"));
    std::fs::write(first.path().join("sentinel"), b"preserve").unwrap();
    assert!(second.git(&["status", "--porcelain"]).unwrap().is_empty());
    assert_eq!(
        std::fs::read(first.path().join("sentinel")).unwrap(),
        b"preserve"
    );
}

// M0-006; FR-19. Treating Git failures as success or leaking output must fail.
#[test]
fn m0_006_failed_git_command_returns_canonical_diagnostic() {
    let repo = GitFixture::new().unwrap();
    let error = repo.git(&["rev-parse", "--verify", "HEAD"]).unwrap_err();
    assert_eq!(error.code, ErrorCode::GitFailed);
    assert!(!error
        .to_string()
        .contains(&repo.path().to_string_lossy().to_string()));
}

// M0-007; FR-01, INV-05. Inherited repository/config redirection must not apply.
#[test]
fn m0_007_ignores_inherited_git_environment() {
    const PROBE: &str = "BATON_TEST_GIT_ENVIRONMENT_PROBE";
    if std::env::var_os(PROBE).is_some() {
        let repo = GitFixture::new().unwrap();
        assert_eq!(repo.git(&["config", "core.autocrlf"]).unwrap(), b"false\n");
        assert!(repo.git(&["status", "--porcelain"]).unwrap().is_empty());
        return;
    }
    // Use a subprocess so parallel tests never observe process-global env changes.
    let outside = tempfile::tempdir().unwrap();
    let config = outside.path().join("hostile.gitconfig");
    std::fs::write(
        &config,
        b"[include]\npath = missing-config\n[core]\nautocrlf = true\n",
    )
    .unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "m0_007_ignores_inherited_git_environment",
            "--nocapture",
        ])
        .env(PROBE, "1")
        .env("GIT_DIR", outside.path().join("nonexistent.git"))
        .env("GIT_WORK_TREE", outside.path())
        .env("GIT_CONFIG_GLOBAL", &config)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.autocrlf")
        .env("GIT_CONFIG_VALUE_0", "true")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!outside.path().join(".git").exists());
    assert_eq!(
        std::fs::read(config).unwrap(),
        b"[include]\npath = missing-config\n[core]\nautocrlf = true\n"
    );
}
