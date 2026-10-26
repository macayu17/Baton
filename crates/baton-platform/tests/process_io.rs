#![cfg(feature = "process")]
use baton_platform::process::{LaunchSpec, ProcessTree};

// M5-NC02; FR-03/12/19, INV-06/09: retain a real nonzero exit without calling it tree quiescence.
#[test]
fn native_root_exit_status_is_retained() {
    let temp = tempfile::tempdir().unwrap();
    let spec = LaunchSpec::new(
        std::path::Path::new(env!("CARGO_BIN_EXE_baton-containment-fixture")),
        temp.path(),
        ["--exit-seven".into()],
    )
    .unwrap();
    let mut tree = ProcessTree::spawn(spec).unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !tree.root_exited().unwrap() {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    for _ in 0..2 {
        assert_eq!(tree.root_exit_status().unwrap().unwrap().code(), Some(7));
    }
}

// M5-B01; FR-03/15, INV-04/09: identical paths cannot disguise changed executable bytes.
#[test]
fn launch_fingerprint_binds_executable_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("owned-program");
    std::fs::write(&program, b"original program").unwrap();
    let original = LaunchSpec::new(&program, temp.path(), [])
        .unwrap()
        .fingerprint();
    std::fs::write(&program, b"changed program!").unwrap();
    let changed = LaunchSpec::new(&program, temp.path(), [])
        .unwrap()
        .fingerprint();
    assert_ne!(
        original, changed,
        "same-path executable changes must change admission identity"
    );
}

// M5-B02; FR-03/15, INV-04/09: a checked Windows executable cannot remain open for writing.
#[cfg(windows)]
#[test]
fn launch_spec_excludes_executable_writes_and_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("owned-program");
    std::fs::write(&program, b"original program").unwrap();
    let spec = LaunchSpec::new(&program, temp.path(), []).unwrap();
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(&program)
        .is_err());
    assert!(std::fs::rename(&program, temp.path().join("renamed-program")).is_err());
    drop(spec);
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .open(&program)
        .unwrap();
    assert!(LaunchSpec::new(&program, temp.path(), []).is_err());
    drop(writer);
    assert_eq!(std::fs::read(&program).unwrap(), b"original program");
}

// M5-B02; the executable handle lasts through native scope ownership and releases on drop.
#[cfg(windows)]
#[test]
fn native_scope_retains_checked_executable_handle_until_drop() {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("owned-program.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    let spec = LaunchSpec::new(
        &program,
        temp.path(),
        [
            "--exact".into(),
            "pipe_child".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ],
    )
    .unwrap()
    .with_piped_stdio();
    let mut tree = ProcessTree::spawn(spec).unwrap();
    let (stdin, mut stdout) = tree.take_stdio().unwrap();
    drop(stdin);
    let mut output = String::new();
    stdout.read_to_string(&mut output).unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while tree.status().unwrap() != baton_platform::process::TreeStatus::Empty {
        assert!(
            std::time::Instant::now() < until,
            "owned child did not drain"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(&program)
        .is_err());
    drop(tree);
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(&program)
        .is_ok());
}

// M5-B03; observed Unix identity/content changes must refuse before native execution.
#[cfg(unix)]
#[test]
fn changed_executable_is_refused_before_spawn() {
    for replace in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("owned-program");
        std::fs::write(&program, b"original program").unwrap();
        let spec = LaunchSpec::new(&program, temp.path(), []).unwrap();
        if replace {
            std::fs::rename(&program, temp.path().join("previous-program")).unwrap();
            std::fs::write(&program, b"original program").unwrap();
        } else {
            std::fs::write(&program, b"changed program!").unwrap();
        }
        assert!(
            matches!(ProcessTree::spawn(spec), Err(error) if error.code == baton_protocol::ErrorCode::IntegrityFailure)
        );
    }
}
use std::io::{BufRead, Read, Write};

// M5-C03; FR-03/15, INV-07: transport choice is bound to native launch and pipes are one-use.
#[test]
fn piped_launch_is_fingerprinted_and_transport_is_taken_once() {
    let temp = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let args = || {
        [
            "--exact".into(),
            "pipe_child".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ]
    };
    let null = LaunchSpec::new(&exe, temp.path(), args()).unwrap();
    let piped = LaunchSpec::new(&exe, temp.path(), args())
        .unwrap()
        .with_piped_stdio();
    assert_ne!(null.fingerprint(), piped.fingerprint());
    let mut null_tree = ProcessTree::spawn(null).unwrap();
    assert!(null_tree.take_stdio().is_err());
    let mut tree = ProcessTree::spawn(piped).unwrap();
    let (mut stdin, mut stdout) = tree.take_stdio().unwrap();
    assert!(tree.take_stdio().is_err());
    stdin.write_all(b"synthetic request\n").unwrap();
    drop(stdin);
    let mut output = String::new();
    stdout.read_to_string(&mut output).unwrap();
    assert!(output.contains("synthetic response: synthetic request"));
}

#[test]
#[ignore = "subprocess entry explicitly invoked by M5-C03"]
fn pipe_child() {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    println!("synthetic response: {}", line.trim());
    if let Ok(value) = std::env::var("BATON_EXPLICIT_TEST") {
        println!("explicit environment: {value}");
    }
}

// M5-C03; explicit environments are deterministic and cannot be swapped after admission.
#[test]
fn explicit_environment_is_validated_fingerprinted_and_applied() {
    let temp = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let spec = || {
        LaunchSpec::new(
            &exe,
            temp.path(),
            [
                "--exact".into(),
                "pipe_child".into(),
                "--ignored".into(),
                "--nocapture".into(),
            ],
        )
        .unwrap()
        .with_piped_stdio()
    };
    for values in [
        vec![("BAD=NAME".into(), "x".into())],
        vec![("Name".into(), "a".into()), ("NAME".into(), "b".into())],
    ] {
        assert!(spec().with_environment(values).is_err());
    }
    let values = vec![
        ("BATON_EXPLICIT_TEST".into(), "synthetic".into()),
        ("BATON_SECOND".into(), "value".into()),
    ];
    let explicit = spec().with_environment(values.clone()).unwrap();
    let reversed = spec()
        .with_environment(values.into_iter().rev().collect())
        .unwrap();
    assert_eq!(explicit.fingerprint(), reversed.fingerprint());
    assert_ne!(spec().fingerprint(), explicit.fingerprint());
    let mut tree = ProcessTree::spawn(explicit).unwrap();
    let (mut stdin, mut stdout) = tree.take_stdio().unwrap();
    stdin.write_all(b"environment check\n").unwrap();
    drop(stdin);
    let mut output = String::new();
    stdout.read_to_string(&mut output).unwrap();
    assert!(output.contains("explicit environment: synthetic"));
}
