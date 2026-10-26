#![cfg(all(windows, feature = "process"))]
use baton_platform::process::{LaunchSpec, ProcessTree, TreeStatus};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    fs,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    time::Duration,
};
use tokio::io::AsyncReadExt;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Probe {
    tier: Option<String>,
    needs_dacl_augmentation: Option<bool>,
    warnings: Vec<serde::de::IgnoredAny>,
    error: Option<serde::de::IgnoredAny>,
    probes: Facts,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    base_container_api_present: bool,
    bfs_compiled_in: bool,
    base_container_supports_deny_paths: bool,
}

fn strict_support(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Probe>(bytes).is_ok_and(|probe| {
        probe.tier.as_deref() == Some("base-container")
            && probe.needs_dacl_augmentation == Some(false)
            && probe.warnings.is_empty()
            && probe.error.is_none()
            && probe.probes.base_container_api_present
            && !probe.probes.bfs_compiled_in
            && probe.probes.base_container_supports_deny_paths
    })
}

struct Case {
    temp: tempfile::TempDir,
    root: PathBuf,
    outside: PathBuf,
    fixture: PathBuf,
    listener: TcpListener,
}
impl Case {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("readonly");
        fs::create_dir(&root).unwrap();
        fs::create_dir(temp.path().join("runtime-home")).unwrap();
        fs::write(root.join("input.txt"), b"baton-owned-input").unwrap();
        let outside = temp.path().join("outside.txt");
        fs::write(&outside, b"outside-owned-fixture").unwrap();
        let fixture = temp.path().join("fixture.exe");
        fs::copy(env!("CARGO_BIN_EXE_baton-containment-fixture"), &fixture).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        Self {
            temp,
            root,
            outside,
            fixture,
            listener,
        }
    }
    fn environment(&self) -> Vec<(OsString, OsString)> {
        vec![
            (
                "LOCALAPPDATA".into(),
                self.temp.path().join("runtime-home").into_os_string(),
            ),
            ("SystemRoot".into(), std::env::var_os("SystemRoot").unwrap()),
            ("BATON_PROBE_ROOT".into(), self.root.as_os_str().to_owned()),
            (
                "BATON_PROBE_OUTSIDE".into(),
                self.outside.as_os_str().to_owned(),
            ),
            (
                "BATON_PROBE_ADDRESS".into(),
                self.listener.local_addr().unwrap().to_string().into(),
            ),
        ]
    }
    fn policy(&self) -> Value {
        let env: Vec<String> = self
            .environment()
            .iter()
            .map(|(k, v)| format!("{}={}", k.to_str().unwrap(), v.to_str().unwrap()))
            .collect();
        json!({"version":"1.0.0","containerId":format!("baton-nc-{}",baton_protocol::Id::new_v4()),"containment":"processcontainer",
            "lifecycle":{"destroyOnExit":true,"preservePolicy":false},
            "process":{"commandLine":format!("\"{}\" --workload",self.fixture.display()),"cwd":self.root,"env":env,"timeout":5000},
            "filesystem":{"readonlyPaths":[self.root,self.fixture],"deniedPaths":[self.outside]},
            "fallback":{"allowDaclMutation":false},
            "network":{"egress":{"default":"deny"},"ingress":{"default":"deny","hostLoopback":"deny"}}})
    }
}

async fn execute(spec: LaunchSpec) -> (i32, Vec<u8>) {
    let mut tree = ProcessTree::spawn(spec).unwrap();
    let (stdin, stdout) = tree.take_stdio().unwrap();
    drop(stdin);
    let stdout = tokio::process::ChildStdout::from_std(stdout).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let mut output = Vec::new();
        stdout.take(65_537).read_to_end(&mut output).await.unwrap();
        assert!(output.len() <= 65_536, "containment output exceeded limit");
        loop {
            if let Some(status) = tree.root_exit_status().unwrap() {
                if tree.status().unwrap() == TreeStatus::Empty {
                    break (status.code().unwrap(), output);
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    result.expect("owned containment probe did not exit/drain before its deadline")
}

fn executor() -> PathBuf {
    std::env::var_os("BATON_CONTAINMENT_BINARY")
        .expect("set explicit checked native executor")
        .into()
}

// M5-NC01; FR-06/15, INV-05/09: no DACL/BFS downgrade or incomplete support is accepted.
#[test]
fn containment_support_refuses_missing_or_weaker_guarantees() {
    let good = json!({"tier":"base-container","needsDaclAugmentation":false,"warnings":[],"error":null,
        "probes":{"baseContainerApiPresent":true,"bfsCompiledIn":false,"baseContainerSupportsDenyPaths":true}});
    assert!(strict_support(&serde_json::to_vec(&good).unwrap()));
    for (field, value) in [
        ("tier", json!("appcontainer-dacl")),
        ("needsDaclAugmentation", json!(true)),
        ("warnings", json!(["degraded"])),
        ("error", json!("refused")),
    ] {
        let mut bad = good.clone();
        bad[field] = value;
        assert!(!strict_support(&serde_json::to_vec(&bad).unwrap()));
    }
    for (field, value) in [
        ("baseContainerApiPresent", false),
        ("bfsCompiledIn", true),
        ("baseContainerSupportsDenyPaths", false),
    ] {
        let mut bad = good.clone();
        bad["probes"][field] = json!(value);
        assert!(!strict_support(&serde_json::to_vec(&bad).unwrap()));
    }
    assert!(!strict_support(b"{}"));
    let duplicate = String::from_utf8(serde_json::to_vec(&good).unwrap())
        .unwrap()
        .replace(
            "\"tier\":\"base-container\"",
            "\"tier\":\"base-container\",\"tier\":\"appcontainer-dacl\"",
        );
    assert!(!strict_support(duplicate.as_bytes()));
}

// M5-NC03; FR-03/06/15, INV-09: the same fixture must demonstrate access without containment.
#[tokio::test]
async fn containment_workload_has_a_real_unconfined_control() {
    let case = Case::new();
    let spec = LaunchSpec::new(&case.fixture, &case.root, ["--workload".into()])
        .unwrap()
        .with_piped_stdio()
        .with_environment(case.environment())
        .unwrap();
    let (code, output) = execute(spec).await;
    assert_eq!(code, 2);
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        report,
        json!({"schema_version":"0.1.0","read_ok":true,"write_denied":false,"outside_denied":false,"network_denied":false})
    );
    assert!(case.root.join("blocked.txt").exists());
    assert!(case.listener.accept().is_ok());
}

// M5-NC01; native read-only host support discovery, with no container/workload launch.
#[tokio::test]
#[ignore = "requires explicit checked Microsoft MXC executor; read-only support probe"]
async fn installed_executor_reports_strict_support_without_launching() {
    let case = Case::new();
    let policy = case.temp.path().join("policy.json");
    fs::write(&policy, serde_json::to_vec(&case.policy()).unwrap()).unwrap();
    let spec = LaunchSpec::new(
        &executor(),
        case.temp.path(),
        ["--probe".into(), policy.into_os_string()],
    )
    .unwrap()
    .with_piped_stdio()
    .with_environment(case.environment())
    .unwrap();
    let (code, output) = execute(spec).await;
    assert_eq!(code, 0);
    assert!(strict_support(&output), "strict native support unavailable");
    assert!(!case.root.join("blocked.txt").exists());
}

// M5-NC04; bounded filesystem and loopback denial, not a source/transfer grant or full cleanup proof.
#[tokio::test]
#[ignore = "requires explicit checked Microsoft MXC executor; launches only an owned temporary fixture"]
async fn installed_executor_confines_owned_native_workload() {
    let case = Case::new();
    drop(TcpStream::connect(case.listener.local_addr().unwrap()).unwrap());
    assert!(case.listener.accept().is_ok());
    let policy = case.temp.path().join("policy.json");
    fs::write(&policy, serde_json::to_vec(&case.policy()).unwrap()).unwrap();
    let request = |args| {
        LaunchSpec::new(&executor(), case.temp.path(), args)
            .unwrap()
            .with_piped_stdio()
            .with_environment(case.environment())
            .unwrap()
    };
    let (code, output) = execute(request(vec![
        "--probe".into(),
        policy.as_os_str().to_owned(),
    ]))
    .await;
    assert_eq!(code, 0);
    assert!(
        strict_support(&output),
        "refusing unsupported or mutating fallback"
    );
    let diagnostics = case.temp.path().join("executor-error.txt");
    let spec = LaunchSpec::new(
        &case.fixture,
        case.temp.path(),
        [
            "--diagnose-executor".into(),
            executor().into_os_string(),
            policy.into_os_string(),
            diagnostics.as_os_str().to_owned(),
        ],
    )
    .unwrap()
    .with_piped_stdio()
    .with_environment(case.environment())
    .unwrap();
    let (code, output) = execute(spec).await;
    assert_eq!(
        code,
        0,
        "contained executor failed: {}",
        fs::read_to_string(&diagnostics).unwrap_or_default()
    );
    assert!(
        fs::read(&diagnostics).unwrap().is_empty(),
        "executor emitted unexpected diagnostics"
    );
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        report,
        json!({"schema_version":"0.1.0","read_ok":true,"write_denied":true,"outside_denied":true,"network_denied":true})
    );
    assert_eq!(
        fs::read(case.root.join("input.txt")).unwrap(),
        b"baton-owned-input"
    );
    assert_eq!(fs::read(&case.outside).unwrap(), b"outside-owned-fixture");
    assert!(!case.root.join("blocked.txt").exists());
    assert!(
        matches!(case.listener.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
    );
}
