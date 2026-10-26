use super::*;
use serde_json::json;

fn key(binding: &RunBinding, seed: &[u8], phase: u8) -> Result<Id, Diagnostic> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"baton-codex-thread-registration-v1:");
    hash.update(&[phase]);
    let binding = serde_json::to_vec(binding).map_err(|_| error(ErrorCode::InvalidInput))?;
    hash.update(&(binding.len() as u64).to_le_bytes());
    hash.update(&binding);
    hash.update(seed);
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.finalize().as_bytes()[..16]);
    Ok(Id::from_bytes(bytes))
}

#[derive(Deserialize)]
struct Method {
    method: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Announcement {
    method: String,
    params: AnnouncementParams,
    #[serde(rename = "emittedAtMs")]
    emitted_at_ms: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnouncementParams {
    thread: Identity,
}
#[derive(Deserialize)]
struct Identity {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteNotice {
    method: String,
    params: RemoteStatus,
    #[serde(rename = "emittedAtMs")]
    emitted_at_ms: Option<i64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RemoteStatus {
    status: String,
    installation_id: String,
    server_name: String,
    environment_id: Option<String>,
}
pub(super) fn remote_disabled(bytes: &[u8]) -> Result<bool, Diagnostic> {
    let method: Method =
        serde_json::from_slice(bytes).map_err(|_| error(ErrorCode::AdapterProtocol))?;
    if method.method.as_deref() != Some("remoteControl/status/changed") {
        return Ok(false);
    }
    let event: RemoteNotice =
        serde_json::from_slice(bytes).map_err(|_| error(ErrorCode::AdapterProtocol))?;
    if event.method != "remoteControl/status/changed"
        || event.params.status != "disabled"
        || event.emitted_at_ms.is_some_and(|time| time < 0)
        || event.params.installation_id.len() > 1024
        || event.params.server_name.len() > 1024
        || event
            .params
            .environment_id
            .as_ref()
            .is_some_and(|id| id.len() > 1024)
    {
        return Err(error(ErrorCode::AdapterProtocol));
    }
    Ok(true)
}
pub(super) fn record_remote_disabled(
    session: &mut Session,
    store: &mut Store,
    actor: &Actor,
    bytes: &[u8],
) -> Result<RunRecord, Diagnostic> {
    let binding = session.binding().clone();
    let operation = key(&binding, bytes, 6)?;
    let sequence = if let Some(prior) =
        store.execution_event(actor, binding.task_id, binding.run_id, operation)?
    {
        let RunObservation::RemoteControlDisabled {
            source_sequence, ..
        } = prior.observation
        else {
            return Err(error(ErrorCode::CorruptStore));
        };
        source_sequence
    } else {
        cursor(store, actor, &binding)?
    };
    session.observe(
        store,
        actor,
        operation,
        RunObservation::RemoteControlDisabled {
            source_sequence: sequence,
            payload_hash: blake3::hash(bytes).to_hex().to_string(),
        },
    )
}

pub(super) fn announcement(bytes: &[u8]) -> Result<Option<String>, Diagnostic> {
    let method: Method =
        serde_json::from_slice(bytes).map_err(|_| error(ErrorCode::AdapterProtocol))?;
    if method.method.as_deref() != Some("thread/started") {
        return Ok(None);
    }
    let event: Announcement =
        serde_json::from_slice(bytes).map_err(|_| error(ErrorCode::AdapterProtocol))?;
    if event.method != "thread/started"
        || !valid_adapter_id(&event.params.thread.id)
        || event.emitted_at_ms.is_some_and(|time| time < 0)
    {
        return Err(error(ErrorCode::AdapterProtocol));
    }
    Ok(Some(event.params.thread.id))
}

pub(super) fn record_announcement(
    session: &mut Session,
    store: &mut Store,
    actor: &Actor,
    thread: &str,
    bytes: &[u8],
) -> Result<RunRecord, Diagnostic> {
    let binding = session.binding().clone();
    let operation = key(&binding, thread.as_bytes(), 5)?;
    let sequence = if let Some(prior) =
        store.execution_event(actor, binding.task_id, binding.run_id, operation)?
    {
        let RunObservation::ThreadAnnounced {
            source_sequence, ..
        } = prior.observation
        else {
            return Err(error(ErrorCode::CorruptStore));
        };
        source_sequence
    } else {
        cursor(store, actor, &binding)?
    };
    session.observe(
        store,
        actor,
        operation,
        RunObservation::ThreadAnnounced {
            source_sequence: sequence,
            thread_id: thread.into(),
            payload_hash: blake3::hash(bytes).to_hex().to_string(),
        },
    )
}

fn cursor(store: &Store, actor: &Actor, binding: &RunBinding) -> Result<u64, Diagnostic> {
    store
        .run(actor, binding.task_id, binding.run_id)?
        .source_cursor
        .checked_add(1)
        .ok_or_else(|| error(ErrorCode::Blocked))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Response {
    Success(Success),
    Failure(Failure),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Success {
    id: AdapterRequestId,
    result: RegistrationReply,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    id: AdapterRequestId,
    #[serde(rename = "error")]
    _error: serde::de::IgnoredAny,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistrationReply {
    thread: Thread,
    model: String,
    model_provider: String,
    cwd: std::path::PathBuf,
    approval_policy: String,
    sandbox: Sandbox,
}
#[derive(Deserialize)]
struct Thread {
    id: String,
    ephemeral: bool,
    turns: Vec<serde::de::IgnoredAny>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Sandbox {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    network_access: bool,
}

fn same_cwd(candidate: &std::path::Path, expected: &std::path::Path) -> bool {
    if !candidate.is_absolute() || !expected.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let drive = |part| match part {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    Some(drive.to_ascii_uppercase())
                }
                _ => None,
            },
            _ => None,
        };
        let mut candidate = candidate.components();
        let mut expected = expected.components();
        let Some(candidate_drive) = drive(candidate.next()) else {
            return false;
        };
        Some(candidate_drive) == drive(expected.next()) && candidate.eq(expected)
    }
    #[cfg(not(windows))]
    {
        candidate == expected
    }
}

fn reply(
    bytes: &[u8],
    request: &AdapterRequestId,
    cwd: &std::path::Path,
    model: &str,
    provider: &str,
) -> Result<String, Diagnostic> {
    let response: Response =
        serde_json::from_slice(bytes).map_err(|_| error(ErrorCode::AdapterProtocol))?;
    let response = match response {
        Response::Failure(failure) if failure.id == *request => {
            return Err(error(ErrorCode::AdapterRejected))
        }
        Response::Success(success) if success.id == *request => success.result,
        _ => return Err(error(ErrorCode::AdapterProtocol)),
    };
    if !valid_adapter_id(&response.thread.id)
        || !response.thread.ephemeral
        || !response.thread.turns.is_empty()
        || response.model != model
        || response.model_provider != provider
        || response.approval_policy != "never"
        || response.sandbox.kind != "readOnly"
        || response.sandbox.network_access
        || !same_cwd(&response.cwd, cwd)
    {
        return Err(error(ErrorCode::AdapterProtocol));
    }
    Ok(response.thread.id)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ConfigResponse {
    Success(ConfigSuccess),
    Failure(Failure),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigSuccess {
    id: AdapterRequestId,
    result: ConfigResult,
}
#[derive(Deserialize)]
struct ConfigResult {
    config: Services,
}
type ServiceMap = std::collections::BTreeMap<String, serde::de::IgnoredAny>;
#[derive(Deserialize)]
struct Services {
    mcp_servers: Option<ServiceMap>,
    hooks: Option<ServiceMap>,
    plugins: Option<ServiceMap>,
    marketplaces: Option<ServiceMap>,
    notify: Option<Vec<serde::de::IgnoredAny>>,
}
fn reported_services(bytes: &[u8], request: &AdapterRequestId) -> Result<bool, Diagnostic> {
    let response: ConfigResponse =
        serde_json::from_slice(bytes).map_err(|_| error(ErrorCode::AdapterProtocol))?;
    let config = match response {
        ConfigResponse::Success(success) if success.id == *request => success.result.config,
        ConfigResponse::Failure(failure) if failure.id == *request => {
            return Err(error(ErrorCode::AdapterRejected))
        }
        _ => return Err(error(ErrorCode::AdapterProtocol)),
    };
    Ok([
        config.mcp_servers,
        config.hooks,
        config.plugins,
        config.marketplaces,
    ]
    .iter()
    .any(|map| map.as_ref().is_some_and(|map| !map.is_empty()))
        || config.notify.is_some_and(|commands| !commands.is_empty()))
}

async fn inspect_config(
    session: &mut Session,
    store: &mut Store,
    actor: &Actor,
    operation: Id,
    reader: &mut BufReader<ChildStdout>,
    writer: &mut ChildStdin,
    deadline: tokio::time::Instant,
) -> Result<(), Diagnostic> {
    let binding = session.binding().clone();
    let request_id = AdapterRequestId::Text(key(&binding, operation.as_bytes(), 10)?.to_string());
    let mut request = serde_json::to_vec(&json!({"id":request_id,"method":"config/read","params":{
        "includeLayers":false,"cwd":session.working_directory()
    }}))
    .map_err(|_| error(ErrorCode::InvalidInput))?;
    request.push(b'\n');
    let effect = key(&binding, &request, 12)?;
    let reserve = Request {
        schema_version: SchemaVersion::V0_1_0,
        operation_id: key(&binding, operation.as_bytes(), 11)?,
        actor_id: actor.id(),
        expected_epoch: binding.epoch,
        action: Action::ObserveRun {
            task_id: binding.task_id,
            run_id: binding.run_id,
            lease_id: binding.lease_id,
            observation: RunObservation::ToolReserved {
                source_sequence: cursor(store, actor, &binding)?,
                effect_id: effect,
                class: EffectClass::LocalReversible,
            },
        },
    };
    let (_, permit) = session.prepare_tool(store, actor, &reserve)?;
    session.authorize_tool(
        store,
        actor,
        permit.ok_or_else(|| error(ErrorCode::Blocked))?,
    )?;
    session.observe(
        store,
        actor,
        key(&binding, operation.as_bytes(), 13)?,
        RunObservation::ToolStarted {
            source_sequence: cursor(store, actor, &binding)?,
            effect_id: effect,
            class: EffectClass::LocalReversible,
        },
    )?;
    tokio::time::timeout_at(deadline, async {
        writer.write_all(&request).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| error(ErrorCode::AdapterTimeout))?
    .map_err(|_| error(ErrorCode::Io))?;
    session.observe(
        store,
        actor,
        key(&binding, operation.as_bytes(), 14)?,
        RunObservation::ToolFinished {
            source_sequence: cursor(store, actor, &binding)?,
            effect_id: effect,
            outcome: ToolOutcome::Succeeded,
        },
    )?;
    let bytes = loop {
        let bytes = tokio::time::timeout_at(deadline, frame(reader))
            .await
            .map_err(|_| error(ErrorCode::AdapterTimeout))??;
        if remote_disabled(&bytes)? {
            record_remote_disabled(session, store, actor, &bytes)?;
        } else {
            break bytes;
        }
    };
    let services_present = reported_services(&bytes, &request_id)?;
    session.observe(
        store,
        actor,
        key(&binding, operation.as_bytes(), 15)?,
        RunObservation::StartupConfigReported {
            source_sequence: cursor(store, actor, &binding)?,
            request_id,
            payload_hash: blake3::hash(&bytes).to_hex().to_string(),
            services_present,
        },
    )?;
    if services_present {
        return Err(error(ErrorCode::ExecutionNotEnabled));
    }
    Ok(())
}

impl Source {
    /// Register the fixed tool in one new ephemeral thread. Does not start a model turn.
    pub async fn start_file_thread(
        self,
        store: &mut Store,
        actor: &Actor,
        operation: Id,
        model: &str,
        provider: &str,
        timeout: Duration,
    ) -> Result<(Self, RunRecord), Diagnostic> {
        let Source {
            session,
            _reader: mut reader,
            _writer: mut writer,
            runtime,
            allow_file_creation,
            experimental,
        } = self;
        let mut guard = Connecting {
            session: Some(session),
            store,
            actor,
            gap: true,
        };
        let attempt = async {
            if timeout.is_zero() || timeout > Duration::from_secs(30) || operation.is_nil()
                || !valid_adapter_id(model) || !valid_adapter_id(provider) { return Err(error(ErrorCode::InvalidInput)); }
            if !experimental || !allow_file_creation { return Err(error(ErrorCode::ExecutionNotEnabled)); }
            let session = guard.session.as_mut().ok_or_else(|| error(ErrorCode::SupervisionFailure))?;
            let binding = session.binding().clone();
            let current = guard.store.run(actor, binding.task_id, binding.run_id)?;
            if binding.actor_id != actor.id() { return Err(error(ErrorCode::Forbidden)); }
            if current.source_thread.is_some() || current.announced_thread.is_some() { return Err(error(ErrorCode::Blocked)); }
            let deadline = tokio::time::Instant::now() + timeout;
            inspect_config(session, guard.store, actor, operation, &mut reader, &mut writer, deadline).await?;
            let cwd = session.working_directory().to_path_buf();
            let request_id = AdapterRequestId::Text(operation.to_string());
            let mut request = serde_json::to_vec(&json!({"id":request_id,"method":"thread/start","params":{
                "cwd":cwd,"model":model,"modelProvider":provider,"sandbox":"read-only","approvalPolicy":"never","ephemeral":true,
                "dynamicTools":[{"type":"function","name":"baton_create_file_v1","description":"Create one new file; never overwrite or execute commands.",
                    "inputSchema":{"type":"object","properties":{"name":{"type":"string","minLength":1,"maxLength":128},"content":{"type":"string","maxLength":16384}},"required":["name","content"],"additionalProperties":false}}]
            }})).map_err(|_| error(ErrorCode::InvalidInput))?;
            request.push(b'\n');
            let payload_hash = blake3::hash(&request).to_hex().to_string();
            let transport = key(&binding, &request, 0)?;
            let effect = key(&binding, &request, 7)?;
            let reserve = Request { schema_version: SchemaVersion::V0_1_0, operation_id: operation,
                actor_id: actor.id(), expected_epoch: binding.epoch, action: Action::ObserveRun {
                    task_id: binding.task_id, run_id: binding.run_id, lease_id: binding.lease_id,
                    observation: RunObservation::ToolReserved { source_sequence: cursor(guard.store, actor, &binding)?,
                        effect_id: transport, class: EffectClass::LocalReversible },
                },
            };
            let (_, permit) = session.prepare_tool(guard.store, actor, &reserve)?;
            session.authorize_tool(guard.store, actor, permit.ok_or_else(|| error(ErrorCode::Blocked))?)?;
            session.observe(guard.store, actor, key(&binding, operation.as_bytes(), 2)?, RunObservation::ToolStarted {
                source_sequence: cursor(guard.store, actor, &binding)?, effect_id: transport, class: EffectClass::LocalReversible,
            })?;
            session.observe(guard.store, actor, key(&binding, operation.as_bytes(), 8)?, RunObservation::ToolStarted {
                source_sequence: cursor(guard.store, actor, &binding)?, effect_id: effect, class: EffectClass::Unknown,
            })?;
            tokio::time::timeout_at(deadline, async { writer.write_all(&request).await?; writer.flush().await })
                .await.map_err(|_| error(ErrorCode::AdapterTimeout))?.map_err(|_| error(ErrorCode::Io))?;
            session.observe(guard.store, actor, key(&binding, operation.as_bytes(), 9)?, RunObservation::ToolFinished {
                source_sequence: cursor(guard.store, actor, &binding)?, effect_id: transport, outcome: ToolOutcome::Succeeded,
            })?;
            let thread = loop {
                let bytes = tokio::time::timeout_at(deadline, frame(&mut reader)).await
                    .map_err(|_| error(ErrorCode::AdapterTimeout))??;
                if remote_disabled(&bytes)? {
                    record_remote_disabled(session, guard.store, actor, &bytes)?;
                } else if let Some(thread) = announcement(&bytes)? {
                    record_announcement(session, guard.store, actor, &thread, &bytes)?;
                } else { break reply(&bytes, &request_id, &cwd, model, provider)?; }
            };
            session.observe(guard.store, actor, key(&binding, operation.as_bytes(), 3)?, RunObservation::ThreadRegistered {
                source_sequence: cursor(guard.store, actor, &binding)?, effect_id: effect, request_id, payload_hash, thread_id: thread,
            })?;
            session.observe(guard.store, actor, key(&binding, operation.as_bytes(), 4)?, RunObservation::ToolFinished {
                source_sequence: cursor(guard.store, actor, &binding)?, effect_id: effect, outcome: ToolOutcome::Succeeded,
            })
        }.await;
        match attempt {
            Ok(record) => Ok((
                Self {
                    session: guard
                        .session
                        .take()
                        .ok_or_else(|| error(ErrorCode::SupervisionFailure))?,
                    _reader: reader,
                    _writer: writer,
                    runtime,
                    allow_file_creation,
                    experimental,
                },
                record,
            )),
            Err(error) => {
                guard.close()?;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // M5-SP02; reported entries reject even when disabled; malformed or ambiguous replies fail closed.
    #[test]
    fn startup_configuration_reply_is_strict_without_storing_service_details() {
        let request = AdapterRequestId::Number(7);
        let good = json!({"id":7,"result":{"config":{"mcp_servers":{},"hooks":null,"plugins":{},"marketplaces":{},"notify":[]},"origins":{},"layers":null}});
        assert!(!reported_services(&serde_json::to_vec(&good).unwrap(), &request).unwrap());
        for field in ["mcp_servers", "hooks", "plugins", "marketplaces", "notify"] {
            let mut configured = good.clone();
            configured["result"]["config"][field] = if field == "notify" {
                json!(["private-command"])
            } else {
                json!({"private-service":{"enabled":false}})
            };
            assert!(
                reported_services(&serde_json::to_vec(&configured).unwrap(), &request).unwrap()
            );
        }
        for bytes in [
            r#"{"id":8,"result":{"config":{}}}"#,
            r#"{"id":7,"result":{"config":{}},"error":null}"#,
            r#"{"id":7,"result":{}}"#,
            r#"{"id":7,"result":{"config":{"mcp_servers":[]}}}"#,
            r#"{"id":7,"result":{"config":{"mcp_servers":{},"mcp_servers":{}}}}"#,
            r#"{"id":7,"result":{"config":{"notify":false}}}"#,
        ] {
            assert_eq!(
                reported_services(bytes.as_bytes(), &request)
                    .unwrap_err()
                    .code,
                ErrorCode::AdapterProtocol
            );
        }
    }
    // M5-R02; vendor identity/policy fields are facts, never accepted by optimistic defaults.
    #[test]
    fn registration_reply_rejects_mixed_ids_and_policy_drift() {
        let root = tempfile::tempdir().unwrap();
        let cwd = std::fs::canonicalize(root.path()).unwrap();
        let good = json!({"id":7,"result":{"thread":{"id":"thread-1","ephemeral":true,"turns":[]},
            "model":"model-1","modelProvider":"provider-1","cwd":cwd,"approvalPolicy":"never",
            "sandbox":{"type":"readOnly","networkAccess":false}}});
        let request = AdapterRequestId::Number(7);
        assert_eq!(
            reply(
                &serde_json::to_vec(&good).unwrap(),
                &request,
                &cwd,
                "model-1",
                "provider-1"
            )
            .unwrap(),
            "thread-1"
        );
        let mut relative = good.clone();
        relative["result"]["cwd"] = json!(".");
        let process_cwd = std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        assert!(
            reply(
                &serde_json::to_vec(&relative).unwrap(),
                &request,
                &process_cwd,
                "model-1",
                "provider-1"
            )
            .is_err(),
            "relative reply paths must not be resolved against the host cwd"
        );
        #[cfg(windows)]
        {
            assert!(!same_cwd(
                std::path::Path::new(r"\\server\share\repo"),
                &cwd
            ));
            assert!(!same_cwd(
                std::path::Path::new(r"\\?\UNC\server\share\repo"),
                &cwd
            ));
            assert!(same_cwd(
                std::path::Path::new(r"C:\repo"),
                std::path::Path::new(r"\\?\C:\repo")
            ));
        }
        for change in [
            "id",
            "mixed",
            "ephemeral",
            "turns",
            "sandbox",
            "network",
            "approval",
            "model",
            "provider",
            "cwd",
        ] {
            let mut changed = good.clone();
            match change {
                "id" => changed["id"] = json!("7"),
                "mixed" => changed["error"] = json!(null),
                "ephemeral" => changed["result"]["thread"]["ephemeral"] = json!(false),
                "turns" => changed["result"]["thread"]["turns"] = json!([{}]),
                "sandbox" => changed["result"]["sandbox"]["type"] = json!("dangerFullAccess"),
                "network" => changed["result"]["sandbox"]["networkAccess"] = json!(true),
                "approval" => changed["result"]["approvalPolicy"] = json!("on-request"),
                "model" => changed["result"]["model"] = json!("other"),
                "provider" => changed["result"]["modelProvider"] = json!("other"),
                _ => changed["result"]["cwd"] = json!(cwd.parent().unwrap()),
            }
            assert!(
                reply(
                    &serde_json::to_vec(&changed).unwrap(),
                    &request,
                    &cwd,
                    "model-1",
                    "provider-1"
                )
                .is_err(),
                "{change}"
            );
        }
        assert_eq!(
            reply(
                br#"{"id":7,"error":{"message":"private vendor message"}}"#,
                &request,
                &cwd,
                "model-1",
                "provider-1"
            )
            .unwrap_err()
            .code,
            ErrorCode::AdapterRejected
        );
    }
    // M5-R02; only disabled remote-control metadata is supported; identities stay transient.
    #[test]
    fn startup_metadata_is_strict_and_does_not_accept_remote_activation() {
        let notice = json!({"method":"remoteControl/status/changed","emittedAtMs":1,"params":{
            "status":"disabled","installationId":"private-id","serverName":"private-name","environmentId":null
        }});
        assert!(remote_disabled(&serde_json::to_vec(&notice).unwrap()).unwrap());
        for state in ["connected", "connecting", "errored"] {
            let mut changed = notice.clone();
            changed["params"]["status"] = json!(state);
            assert!(remote_disabled(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
        let mut changed = notice;
        changed["emittedAtMs"] = json!(-1);
        assert!(remote_disabled(&serde_json::to_vec(&changed).unwrap()).is_err());
        let announced = json!({"method":"thread/started","emittedAtMs":1,"params":{"thread":{"id":"thread-1","preview":"private text"}}});
        assert_eq!(
            announcement(&serde_json::to_vec(&announced).unwrap())
                .unwrap()
                .as_deref(),
            Some("thread-1")
        );
        let mut changed = announced;
        changed["id"] = json!(7);
        assert!(announcement(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
}
