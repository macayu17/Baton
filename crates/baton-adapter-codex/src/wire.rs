use baton_protocol::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "method", content = "params", deny_unknown_fields)]
enum Notification {
    #[serde(rename = "turn/started")]
    TurnStarted(TurnParams),
    #[serde(rename = "turn/completed")]
    TurnCompleted(TurnParams),
    #[serde(rename = "item/started")]
    ItemStarted(ItemParams),
    #[serde(rename = "item/completed")]
    ItemCompleted(ItemParams),
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TurnParams {
    thread_id: String,
    turn: Turn,
}
#[derive(Deserialize)]
struct Turn {
    id: String,
    status: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItemParams {
    thread_id: String,
    turn_id: String,
    item: Item,
    started_at_ms: Option<i64>,
    completed_at_ms: Option<i64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    status: Option<String>,
    exit_code: Option<i32>,
    tool: Option<String>,
}
fn invalid() -> Diagnostic {
    Diagnostic::new(ErrorCode::AdapterProtocol)
}
fn scoped_id(
    binding: &RunBinding,
    adapter: &AgentMetadata,
    thread: &str,
    turn: &str,
    item: &str,
    phase: u8,
) -> Result<Id, Diagnostic> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"baton-codex-observation-v1:");
    hash.update(&[phase]);
    let binding = serde_json::to_vec(binding).map_err(|_| invalid())?;
    hash.update(&(binding.len() as u64).to_le_bytes());
    hash.update(&binding);
    for value in [&adapter.name, &adapter.version, thread, turn, item] {
        hash.update(&(value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    }
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.finalize().as_bytes()[..16]);
    Ok(Id::from_bytes(bytes))
}
pub(crate) fn operation_id(binding: &RunBinding, event: &AdapterEvent) -> Result<Id, Diagnostic> {
    let (item, phase) = match &event.event {
        AdapterEventKind::DynamicToolExecution(callback) => {
            let key = match callback.phase {
                CallbackPhase::Planned => 5,
                CallbackPhase::Written => 6,
                CallbackPhase::Resolved => 7,
            };
            return control_operation_id(
                binding,
                &event.adapter,
                &event.thread_id,
                &callback.request_id,
                key,
            );
        }
        AdapterEventKind::CallbackRefusal {
            request_id, phase, ..
        } => {
            let key = match phase {
                CallbackPhase::Planned => 5,
                CallbackPhase::Written => 6,
                CallbackPhase::Resolved => 7,
            };
            return control_operation_id(
                binding,
                &event.adapter,
                &event.thread_id,
                request_id,
                key,
            );
        }
        AdapterEventKind::TurnStarted => ("", 0),
        AdapterEventKind::TurnCompleted { .. } => ("", 1),
        AdapterEventKind::ToolStarted { item_id, .. }
        | AdapterEventKind::PassiveStarted { item_id, .. } => (item_id.as_str(), 2),
        AdapterEventKind::ToolCompleted { item_id, .. }
        | AdapterEventKind::PassiveCompleted { item_id, .. } => (item_id.as_str(), 3),
        AdapterEventKind::ApprovalCancellationPlanned { request_id, .. } => {
            return control_operation_id(binding, &event.adapter, &event.thread_id, request_id, 5)
        }
        AdapterEventKind::ApprovalCancellationWritten { request_id, .. } => {
            return control_operation_id(binding, &event.adapter, &event.thread_id, request_id, 6)
        }
        AdapterEventKind::ApprovalResolutionReported { request_id } => {
            return control_operation_id(binding, &event.adapter, &event.thread_id, request_id, 7)
        }
        AdapterEventKind::TurnInterruptPlanned { request_id } => {
            return control_operation_id(binding, &event.adapter, &event.thread_id, request_id, 8)
        }
        AdapterEventKind::TurnInterruptWritten { request_id } => {
            return control_operation_id(binding, &event.adapter, &event.thread_id, request_id, 9)
        }
        AdapterEventKind::TurnInterruptAcknowledged { request_id } => {
            return control_operation_id(binding, &event.adapter, &event.thread_id, request_id, 10)
        }
    };
    scoped_id(
        binding,
        &event.adapter,
        &event.thread_id,
        &event.turn_id,
        item,
        phase,
    )
}
pub(crate) fn control_operation_id(
    binding: &RunBinding,
    adapter: &AgentMetadata,
    thread: &str,
    request_id: &AdapterRequestId,
    phase: u8,
) -> Result<Id, Diagnostic> {
    scoped_id(binding, adapter, thread, "", &request_id.key(), phase)
}
pub(crate) enum Incoming {
    Callback(AdapterEvent),
    Event,
    Approval(AdapterEvent),
    Resolution(AdapterRequestId),
    InterruptReply(AdapterRequestId),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    method: Option<String>,
    id: Option<AdapterRequestId>,
    #[serde(rename = "params")]
    _params: Option<serde::de::IgnoredAny>,
    #[serde(rename = "result")]
    _result: Option<serde::de::IgnoredAny>,
    #[serde(rename = "error")]
    _error: Option<serde::de::IgnoredAny>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyReply {}
#[derive(Deserialize)]
#[serde(untagged)]
enum InterruptReply {
    Success(InterruptSuccess),
    Failure(InterruptFailure),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InterruptSuccess {
    id: AdapterRequestId,
    #[serde(rename = "result")]
    _result: EmptyReply,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InterruptFailure {
    id: AdapterRequestId,
    #[serde(rename = "error")]
    _error: serde::de::IgnoredAny,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalRequest {
    id: AdapterRequestId,
    method: String,
    params: ApprovalParams,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallbackRequest<T> {
    id: AdapterRequestId,
    method: String,
    params: T,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DynamicParams {
    thread_id: String,
    turn_id: String,
    call_id: String,
    tool: String,
    namespace: Option<String>,
    #[serde(rename = "arguments")]
    _arguments: serde::de::IgnoredAny,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InputParams {
    thread_id: String,
    turn_id: String,
    item_id: String,
    #[serde(rename = "isBlocking")]
    _blocking: bool,
    questions: Vec<InputQuestion>,
}
#[derive(Deserialize)]
struct InputQuestion {
    id: String,
    #[serde(rename = "header")]
    _header: String,
    #[serde(rename = "question")]
    _question: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalParams {
    thread_id: String,
    turn_id: String,
    item_id: String,
    started_at_ms: i64,
    available_decisions: Option<Vec<serde_json::Value>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolved {
    #[serde(rename = "method")]
    _method: String,
    params: ResolvedParams,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolvedParams {
    thread_id: String,
    request_id: AdapterRequestId,
}
pub(crate) fn incoming(
    bytes: &[u8],
    adapter: &AgentMetadata,
    thread: &str,
) -> Result<Incoming, Diagnostic> {
    let header: Header = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if header.method.is_none() {
        let reply: InterruptReply = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        return match reply {
            InterruptReply::Success(reply) if reply.id.valid() => {
                Ok(Incoming::InterruptReply(reply.id))
            }
            InterruptReply::Failure(reply) if reply.id.valid() => {
                Err(Diagnostic::new(ErrorCode::AdapterRejected))
            }
            _ => Err(invalid()),
        };
    }
    if header.id.is_some() {
        if matches!(
            header.method.as_deref(),
            Some("item/tool/call" | "item/tool/requestUserInput")
        ) {
            let (request_id, thread_id, turn_id, correlation_id, callback) =
                if header.method.as_deref() == Some("item/tool/call") {
                    let r: CallbackRequest<DynamicParams> =
                        serde_json::from_slice(bytes).map_err(|_| invalid())?;
                    if r.method != "item/tool/call"
                        || !valid_adapter_id(&r.params.tool)
                        || r.params
                            .namespace
                            .as_ref()
                            .is_some_and(|n| !valid_adapter_id(n))
                    {
                        return Err(invalid());
                    }
                    (
                        r.id,
                        r.params.thread_id,
                        r.params.turn_id,
                        r.params.call_id,
                        CallbackKind::DynamicTool,
                    )
                } else {
                    let r: CallbackRequest<InputParams> =
                        serde_json::from_slice(bytes).map_err(|_| invalid())?;
                    if r.method != "item/tool/requestUserInput"
                        || r.params.questions.is_empty()
                        || r.params.questions.len() > 3
                        || r.params.questions.iter().any(|q| !valid_adapter_id(&q.id))
                    {
                        return Err(invalid());
                    }
                    (
                        r.id,
                        r.params.thread_id,
                        r.params.turn_id,
                        r.params.item_id,
                        CallbackKind::UserInput,
                    )
                };
            if thread_id != thread || adapter.name != "codex" {
                return Err(invalid());
            }
            let event = AdapterEvent {
                adapter: adapter.clone(),
                thread_id,
                turn_id,
                event: AdapterEventKind::CallbackRefusal {
                    request_id,
                    correlation_id,
                    callback,
                    payload_hash: blake3::hash(bytes).to_hex().to_string(),
                    phase: CallbackPhase::Planned,
                },
            };
            event.validate().map_err(|_| invalid())?;
            return Ok(Incoming::Callback(event));
        }
        let request: ApprovalRequest = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        let tool = match request.method.as_str() {
            "item/commandExecution/requestApproval" => AdapterToolKind::CommandExecution,
            "item/fileChange/requestApproval" => AdapterToolKind::FileChange,
            _ => return Err(invalid()),
        };
        if request.params.thread_id != thread
            || request.params.started_at_ms < 0
            || adapter.name != "codex"
        {
            return Err(invalid());
        }
        if request
            .params
            .available_decisions
            .as_ref()
            .is_some_and(|decisions| {
                decisions.len() > 64
                    || !decisions
                        .iter()
                        .any(|decision| decision.as_str() == Some("cancel"))
            })
        {
            return Err(invalid());
        }
        let event = AdapterEvent {
            adapter: adapter.clone(),
            thread_id: request.params.thread_id,
            turn_id: request.params.turn_id,
            event: AdapterEventKind::ApprovalCancellationPlanned {
                request_id: request.id,
                item_id: request.params.item_id,
                tool,
            },
        };
        event.validate().map_err(|_| invalid())?;
        return Ok(Incoming::Approval(event));
    }
    if header.method.as_deref() == Some("serverRequest/resolved") {
        let resolved: Resolved = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        if resolved.params.thread_id != thread || !resolved.params.request_id.valid() {
            return Err(invalid());
        }
        return Ok(Incoming::Resolution(resolved.params.request_id));
    }
    Ok(Incoming::Event)
}
pub(crate) fn cancel_reply(request_id: &AdapterRequestId) -> Result<Vec<u8>, Diagnostic> {
    if !request_id.valid() {
        return Err(invalid());
    }
    let mut bytes =
        serde_json::to_vec(&serde_json::json!({"id":request_id,"result":{"decision":"cancel"}}))
            .map_err(|_| invalid())?;
    bytes.push(b'\n');
    Ok(bytes)
}
pub(crate) fn dynamic_failure(request_id: &AdapterRequestId) -> Result<Vec<u8>, Diagnostic> {
    if !request_id.valid() {
        return Err(invalid());
    }
    let mut bytes = serde_json::to_vec(&serde_json::json!({"id": request_id, "result":{"success":false,"contentItems":[{"type":"inputText","text":"BATON_EXECUTION_NOT_ENABLED"}]}})).map_err(|_| invalid())?;
    bytes.push(b'\n');
    Ok(bytes)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileCreation {
    pub name: String,
    pub content: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreationParams {
    thread_id: String,
    turn_id: String,
    call_id: String,
    tool: String,
    namespace: Option<String>,
    arguments: FileCreation,
}
pub(crate) fn file_creation(bytes: &[u8]) -> Result<FileCreation, Diagnostic> {
    let request: CallbackRequest<CreationParams> =
        serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let params = request.params;
    if request.method != "item/tool/call"
        || !request.id.valid()
        || params.tool != "baton_create_file_v1"
        || params.namespace.is_some()
        || ![&params.thread_id, &params.turn_id, &params.call_id]
            .into_iter()
            .all(|id| valid_adapter_id(id))
        || params.arguments.content.len() > 16_384
    {
        return Err(invalid());
    }
    Ok(params.arguments)
}
pub(crate) fn creation_reply(
    request_id: &AdapterRequestId,
    digest: &str,
) -> Result<Vec<u8>, Diagnostic> {
    if !request_id.valid() || !valid_evidence_hash(digest) {
        return Err(invalid());
    }
    let mut bytes = serde_json::to_vec(&serde_json::json!({"id":request_id,"result":{
        "success":true,"contentItems":[{"type":"inputText","text":format!("BLAKE3:{digest}")}]
    }}))
    .map_err(|_| invalid())?;
    bytes.push(b'\n');
    Ok(bytes)
}
pub(crate) fn interrupt_id(
    binding: &RunBinding,
    adapter: &AgentMetadata,
    thread: &str,
    turn: &str,
) -> Result<AdapterRequestId, Diagnostic> {
    if adapter.name != "codex" || !valid_adapter_id(thread) || !valid_adapter_id(turn) {
        return Err(invalid());
    }
    Ok(AdapterRequestId::Text(
        scoped_id(binding, adapter, thread, turn, "", 11)?.to_string(),
    ))
}
pub(crate) fn interrupt_request(
    request_id: &AdapterRequestId,
    thread: &str,
    turn: &str,
) -> Result<Vec<u8>, Diagnostic> {
    if !request_id.valid() || !valid_adapter_id(thread) || !valid_adapter_id(turn) {
        return Err(invalid());
    }
    let mut bytes = serde_json::to_vec(&serde_json::json!({"id": request_id, "method":"turn/interrupt", "params":{"threadId":thread,"turnId":turn}})).map_err(|_| invalid())?;
    bytes.push(b'\n');
    Ok(bytes)
}
pub(crate) fn normalize(
    bytes: &[u8],
    binding: &RunBinding,
    adapter: &AgentMetadata,
    thread: &str,
) -> Result<AdapterEvent, Diagnostic> {
    if adapter.name != "codex" || !valid_adapter_id(thread) {
        return Err(invalid());
    }
    let notification: Notification = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let completed = matches!(
        notification,
        Notification::TurnCompleted(_) | Notification::ItemCompleted(_)
    );
    let (thread_id, turn_id, event) = match notification {
        Notification::TurnStarted(params) | Notification::TurnCompleted(params) => {
            let event = if completed {
                let outcome = match params.turn.status.as_str() {
                    "completed" => ToolOutcome::Succeeded,
                    "failed" => ToolOutcome::Failed,
                    "interrupted" => ToolOutcome::Interrupted,
                    _ => return Err(invalid()),
                };
                AdapterEventKind::TurnCompleted { outcome }
            } else {
                if params.turn.status != "inProgress" {
                    return Err(invalid());
                }
                AdapterEventKind::TurnStarted
            };
            (params.thread_id, params.turn.id, event)
        }
        Notification::ItemStarted(params) | Notification::ItemCompleted(params) => {
            let timestamp = if completed {
                params.completed_at_ms
            } else {
                params.started_at_ms
            };
            if timestamp.is_none_or(|value| value < 0) {
                return Err(invalid());
            }
            let passive = match params.item.kind.as_str() {
                "dynamicToolCall" => {
                    if params
                        .item
                        .tool
                        .as_ref()
                        .is_none_or(|tool| !valid_adapter_id(tool))
                        || (!completed && params.item.status.as_deref() != Some("inProgress"))
                        || (completed
                            && !matches!(
                                params.item.status.as_deref(),
                                Some("completed" | "failed")
                            ))
                    {
                        return Err(invalid());
                    }
                    Some(PassiveItemKind::ClientToolCall)
                }
                "userMessage" => Some(PassiveItemKind::UserMessage),
                "agentMessage" => Some(PassiveItemKind::AgentMessage),
                "reasoning" => Some(PassiveItemKind::Reasoning),
                "plan" => Some(PassiveItemKind::Plan),
                "contextCompaction" => Some(PassiveItemKind::ContextCompaction),
                _ => None,
            };
            let event = if let Some(item) = passive {
                if params.item.exit_code.is_some() {
                    return Err(invalid());
                }
                if completed {
                    AdapterEventKind::PassiveCompleted {
                        item_id: params.item.id,
                        item,
                    }
                } else {
                    AdapterEventKind::PassiveStarted {
                        item_id: params.item.id,
                        item,
                    }
                }
            } else {
                let status = params.item.status.as_deref().ok_or_else(invalid)?;
                let tool = match params.item.kind.as_str() {
                    "commandExecution" => AdapterToolKind::CommandExecution,
                    "fileChange" => AdapterToolKind::FileChange,
                    _ => return Err(invalid()),
                };
                let effect_id = scoped_id(
                    binding,
                    adapter,
                    &params.thread_id,
                    &params.turn_id,
                    &params.item.id,
                    4,
                )?;
                if completed {
                    let outcome = match status {
                        "completed" if tool == AdapterToolKind::CommandExecution => {
                            match params.item.exit_code {
                                Some(0) => ToolOutcome::Succeeded,
                                Some(_) => ToolOutcome::Failed,
                                None => return Err(invalid()),
                            }
                        }
                        "completed" => ToolOutcome::Succeeded,
                        "failed" => ToolOutcome::Failed,
                        "declined" => ToolOutcome::Interrupted,
                        _ => return Err(invalid()),
                    };
                    if tool == AdapterToolKind::FileChange && params.item.exit_code.is_some() {
                        return Err(invalid());
                    }
                    AdapterEventKind::ToolCompleted {
                        item_id: params.item.id,
                        effect_id,
                        tool,
                        outcome,
                        exit_code: params.item.exit_code,
                    }
                } else {
                    if status != "inProgress" || params.item.exit_code.is_some() {
                        return Err(invalid());
                    }
                    AdapterEventKind::ToolStarted {
                        item_id: params.item.id,
                        effect_id,
                        tool,
                    }
                }
            };
            (params.thread_id, params.turn_id, event)
        }
    };
    if thread_id != thread {
        return Err(invalid());
    }
    let event = AdapterEvent {
        adapter: adapter.clone(),
        thread_id,
        turn_id,
        event,
    };
    event.validate().map_err(|_| invalid())?;
    Ok(event)
}
#[cfg(test)]
mod tests {
    // M5-X02; the opt-in host recipe is versioned, strict and bounded.
    #[test]
    fn file_creation_recipe_rejects_extra_fields_and_unsupported_tools() {
        let good = serde_json::json!({"id":7,"method":"item/tool/call","params":{
            "threadId":"thread-1","turnId":"turn-1","callId":"call-1",
            "tool":"baton_create_file_v1","arguments":{"name":"new.txt","content":"private text"}
        }});
        let recipe = super::file_creation(&serde_json::to_vec(&good).unwrap()).unwrap();
        assert_eq!(recipe.name, "new.txt");
        assert_eq!(recipe.content, "private text");
        for change in ["extra", "tool", "namespace", "large", "string"] {
            let mut changed = good.clone();
            match change {
                "extra" => {
                    changed["params"]["arguments"]["command"] = serde_json::json!("arbitrary")
                }
                "tool" => changed["params"]["tool"] = serde_json::json!("shell"),
                "namespace" => changed["params"]["namespace"] = serde_json::json!("other"),
                "large" => {
                    changed["params"]["arguments"]["content"] =
                        serde_json::json!("x".repeat(16_385))
                }
                _ => changed["params"]["arguments"] = serde_json::json!("not an object"),
            }
            assert!(
                super::file_creation(&serde_json::to_vec(&changed).unwrap()).is_err(),
                "{change}"
            );
        }
    }
    use super::*;
    use serde_json::json;
    // M5-D02/U02; callback contents stay transient; no answer, permission or success is invented.
    #[test]
    fn callback_requests_are_scoped_hashed_and_refused() {
        let adapter = AgentMetadata {
            name: "codex".into(),
            version: "0.164.0".into(),
        };
        for request in [
            json!({"id":7,"method":"item/tool/call","params":{"threadId":"thread-1","turnId":"turn-1","callId":"call-1","tool":"baton_probe","arguments":{"private":"secret payload"}}}),
            json!({"id":"q-1","method":"item/tool/requestUserInput","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"item-1","isBlocking":true,"questions":[{"id":"q","header":"Choice","question":"private question"}]}}),
        ] {
            let bytes = serde_json::to_vec(&request).unwrap();
            let Incoming::Callback(event) = incoming(&bytes, &adapter, "thread-1").unwrap() else {
                panic!("not a callback");
            };
            let persisted = serde_json::to_string(&event).unwrap();
            assert!(!persisted.contains("secret payload"));
            assert!(!persisted.contains("private question"));
            let AdapterEventKind::CallbackRefusal {
                payload_hash,
                phase,
                ..
            } = event.event
            else {
                panic!();
            };
            assert_eq!(payload_hash, blake3::hash(&bytes).to_hex().as_str());
            assert_eq!(phase, CallbackPhase::Planned);
            assert!(incoming(&bytes, &adapter, "other-thread").is_err());
        }
        let reply: serde_json::Value =
            serde_json::from_slice(&dynamic_failure(&AdapterRequestId::Number(7)).unwrap())
                .unwrap();
        assert_eq!(
            reply,
            json!({"id":7,"result":{"success":false,"contentItems":[{"type":"inputText","text":"BATON_EXECUTION_NOT_ENABLED"}]}})
        );
        assert!(incoming(br#"{"id":7,"method":"item/tool/requestUserInput","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"i","questions":[]}}"#, &adapter, "thread-1").is_err());
    }
    // M5-I03; FR-03/15, INV-09: only an empty success object is an interrupt acknowledgement.
    #[test]
    fn interrupt_reply_rejects_ambiguous_or_nonempty_success() {
        let adapter = AgentMetadata {
            name: "codex".into(),
            version: "0.164.0".into(),
        };
        for bad in [
            r#"{"id":"i","result":{},"error":null}"#,
            r#"{"id":"i","result":{"stopped":true}}"#,
            r#"{"id":"i","result":null}"#,
            r#"{"id":"i","id":"j","result":{}}"#,
            r#"{"id":"i","result":{},"params":{}}"#,
        ] {
            assert!(
                incoming(bad.as_bytes(), &adapter, "thread-1").is_err(),
                "accepted {bad}"
            );
        }
        assert!(
            matches!(incoming(br#"{"id":"i","result":{}}"#, &adapter, "thread-1").unwrap(), Incoming::InterruptReply(AdapterRequestId::Text(id)) if id == "i")
        );
        let binding = binding();
        let id = interrupt_id(&binding, &adapter, "thread-1", "turn-1").unwrap();
        assert_ne!(
            id,
            interrupt_id(&binding, &adapter, "thread-1", "turn-2").unwrap()
        );
        let request: serde_json::Value =
            serde_json::from_slice(&interrupt_request(&id, "thread-1", "turn-1").unwrap()).unwrap();
        assert_eq!(
            request,
            json!({"id": id, "method":"turn/interrupt", "params":{"threadId":"thread-1","turnId":"turn-1"}})
        );
    }
    fn binding() -> RunBinding {
        RunBinding {
            task_id: Id::new_v4(),
            run_id: Id::new_v4(),
            actor_id: Id::new_v4(),
            lease_id: Id::new_v4(),
            epoch: 1,
        }
    }
    // M5-A02/P01; supported refusals and passive metadata never authorize execution or store text.
    #[test]
    fn approval_is_identified_and_passive_text_is_omitted() {
        let adapter = AgentMetadata {
            name: "codex".into(),
            version: "0.164.0".into(),
        };
        let approval=serde_json::to_vec(&json!({"id":9,"method":"item/commandExecution/requestApproval","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"item-1","startedAtMs":1,"command":"private command"}})).unwrap();
        assert!(matches!(
            incoming(&approval, &adapter, "thread-1").unwrap(),
            Incoming::Approval(_)
        ));
        let passive=serde_json::to_vec(&json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","completedAtMs":2,
            "item":{"type":"agentMessage","id":"message-1","text":"private message"}}})).unwrap();
        let event = normalize(&passive, &binding(), &adapter, "thread-1").unwrap();
        assert!(matches!(
            event.event,
            AdapterEventKind::PassiveCompleted {
                item: PassiveItemKind::AgentMessage,
                ..
            }
        ));
        assert!(!serde_json::to_string(&event).unwrap().contains("private"));
    }

    // M5-A02; approval parser rejects mismatched, malformed and unsupported scope without granting anything.
    #[test]
    fn approval_ids_and_scopes_are_strict_and_reply_is_only_cancel() {
        let adapter = AgentMetadata {
            name: "codex".into(),
            version: "0.164.0".into(),
        };
        let good = json!({"id":7,"method":"item/commandExecution/requestApproval","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"item-1","startedAtMs":1}});
        for index in 0..5 {
            let mut value = good.clone();
            match index {
                0 => value["params"]["threadId"] = json!("other"),
                1 => value["id"] = json!({"bad":"id"}),
                2 => value["params"]["startedAtMs"] = json!(-1),
                3 => value["method"] = json!("item/permissions/requestApproval"),
                _ => value["params"]["availableDecisions"] = json!(["acceptForSession"]),
            }
            assert!(incoming(&serde_json::to_vec(&value).unwrap(), &adapter, "thread-1").is_err());
        }
        for id in [
            AdapterRequestId::Number(i64::MIN),
            AdapterRequestId::Text("request-1".into()),
        ] {
            let reply: serde_json::Value =
                serde_json::from_slice(&cancel_reply(&id).unwrap()).unwrap();
            assert_eq!(reply, json!({"id":id,"result":{"decision":"cancel"}}));
        }
    }
    // M5-E01/E03; keep vendor exit facts and opaque correlation without persisting raw outputs.
    #[test]
    fn command_failure_is_normalized_without_command_or_stdout() {
        let bytes=serde_json::to_vec(&json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","completedAtMs":1,
            "item":{"type":"commandExecution","id":"item-1","status":"completed","exitCode":7,"command":"secret command","aggregatedOutput":"secret output"}}})).unwrap();
        let event = normalize(
            &bytes,
            &binding(),
            &AgentMetadata {
                name: "codex".into(),
                version: "0.164.0".into(),
            },
            "thread-1",
        )
        .unwrap();
        assert!(matches!(
            event.event,
            AdapterEventKind::ToolCompleted {
                outcome: ToolOutcome::Failed,
                exit_code: Some(7),
                ..
            }
        ));
        assert!(!serde_json::to_string(&event).unwrap().contains("secret"));
    }

    // M5-E02; unsupported messages and contradictory facts must become explicit capture failure.
    #[test]
    fn wrong_or_unsupported_wire_shapes_fail_closed() {
        let adapter = AgentMetadata {
            name: "codex".into(),
            version: "0.164.0".into(),
        };
        let binding = binding();
        let good = json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","completedAtMs":1,
            "item":{"type":"commandExecution","id":"item-1","status":"completed","exitCode":0}}});
        for mut value in [good.clone(), good.clone(), good.clone(), good.clone()]
            .into_iter()
            .enumerate()
        {
            match value.0 {
                0 => value.1["params"]["threadId"] = json!("wrong-thread"),
                1 => value.1["params"]["item"]["exitCode"] = json!(null),
                2 => value.1["params"]["item"]["type"] = json!("unsupportedTool"),
                _ => value.1["id"] = json!(1),
            }
            assert!(normalize(
                &serde_json::to_vec(&value.1).unwrap(),
                &binding,
                &adapter,
                "thread-1"
            )
            .is_err());
        }
        let duplicate=br#"{"method":"turn/started","params":{"threadId":"thread-1","threadId":"other","turn":{"id":"turn-1","status":"inProgress"}}}"#;
        assert!(normalize(duplicate, &binding, &adapter, "thread-1").is_err());
    }
}
