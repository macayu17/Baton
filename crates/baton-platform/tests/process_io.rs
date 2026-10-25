#![cfg(feature = "process")]
use baton_platform::process::{LaunchSpec, ProcessTree};
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
