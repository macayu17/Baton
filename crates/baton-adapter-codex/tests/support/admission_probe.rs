//! Opt-in native compatibility probe; model traffic terminates at a loopback fixture.
use super::*;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};

async fn request_body(reader: &mut (impl AsyncRead + Unpin)) -> Result<Vec<u8>, &'static str> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= 16_384 {
            return Err("oversized HTTP header");
        }
        header.push(
            reader
                .read_u8()
                .await
                .map_err(|_| "truncated HTTP header")?,
        );
    }
    let header = std::str::from_utf8(&header).map_err(|_| "non-UTF8 HTTP header")?;
    if !header.starts_with("POST /v1/responses HTTP/1.1\r\n") {
        return Err("unexpected provider route");
    }
    let mut length = None;
    for line in header.lines().skip(1).filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once(':').ok_or("malformed HTTP header")?;
        if key.eq_ignore_ascii_case("authorization") {
            return Err("unexpected credential header");
        }
        if key.eq_ignore_ascii_case("transfer-encoding")
            || key.eq_ignore_ascii_case("content-encoding")
        {
            return Err("unsupported HTTP encoding");
        }
        if key.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err("duplicate HTTP length");
            }
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "invalid HTTP length")?,
            );
        }
    }
    let length = length
        .filter(|size| *size <= 2_097_152)
        .ok_or("missing or excessive HTTP length")?;
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|_| "truncated HTTP body")?;
    Ok(body)
}

async fn send(writer: &mut tokio::process::ChildStdin, message: Value) {
    let mut bytes = serde_json::to_vec(&message).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
    writer.flush().await.unwrap();
}
async fn frame(reader: &mut BufReader<tokio::process::ChildStdout>) -> Value {
    let mut bytes = Vec::new();
    loop {
        assert!(bytes.len() < 65_536, "oversized native probe frame");
        let byte = reader.read_u8().await.expect("native probe EOF");
        if byte == b'\n' {
            break;
        }
        bytes.push(byte);
    }
    serde_json::from_slice(&bytes).expect("invalid native probe JSON")
}

// M5-G00; FR-15/16, INV-09: the probe itself refuses credentials, alternate routes and oversized data.
#[tokio::test]
async fn local_provider_reader_is_bounded_and_rejects_credentials() {
    for bad in [
        b"POST /v1/responses HTTP/1.1\r\nContent-Length: 2097153\r\n\r\n".as_slice(),
        b"POST /v1/responses HTTP/1.1\r\nAuthorization: synthetic\r\nContent-Length: 0\r\n\r\n",
        b"POST /other HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        b"POST /v1/responses HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
        b"POST /v1/responses HTTP/1.1\r\nContent-Length: 2\r\n\r\nx",
    ] {
        assert!(request_body(&mut &bad[..]).await.is_err());
    }
    assert_eq!(
        request_body(&mut &b"POST /v1/responses HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}"[..])
            .await
            .unwrap(),
        b"{}"
    );
}

// M5-G01; FR-03/06/15, INV-04/06/09: runtime inventory is necessary, never sufficient writer proof.
#[tokio::test]
#[ignore = "requires explicit native Codex binary; loopback provider only, no real model"]
async fn installed_codex_advertises_only_baton_controlled_tools() {
    let names = native_probe(ProbeFlow::Inventory).await;
    assert_eq!(
        names,
        ["baton_probe"],
        "uncontrolled tools remain; writer admission must stay disabled"
    );
}

// M5-D06; actual Codex consumes the failure and forwards it to the local fixture, without execution.
#[tokio::test]
#[ignore = "requires explicit native Codex binary; synthetic loopback provider only"]
async fn installed_codex_dynamic_refusal_reaches_local_provider() {
    let names = native_probe(ProbeFlow::DynamicRefusal).await;
    assert!(names.iter().any(|name| name == "baton_probe"));
}

// M5-U04; actual client input request is observed, no answer is synthesized, owned scope stops.
#[tokio::test]
#[ignore = "requires explicit native Codex binary; synthetic loopback provider only"]
async fn installed_codex_input_request_can_be_stopped_without_answer() {
    let names = native_probe(ProbeFlow::InputBlocked).await;
    assert!(names.iter().any(|name| name == "request_user_input"));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProbeFlow {
    Inventory,
    DynamicRefusal,
    InputBlocked,
}

async fn respond(socket: &mut tokio::net::TcpStream, events: &[Value]) {
    let bytes: String = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect();
    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).as_bytes()).await.unwrap();
    socket.write_all(bytes.as_bytes()).await.unwrap();
    socket.shutdown().await.unwrap();
}

async fn native_probe(flow: ProbeFlow) -> Vec<String> {
    let binary =
        std::env::var_os("BATON_CODEX_BINARY").expect("set explicit native Codex executable");
    let mut f = Fixture::new();
    let home = f.temp.path().join("state/inventory-home");
    std::fs::create_dir(&home).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, peer) = listener.accept().await.unwrap();
        assert!(peer.ip().is_loopback());
        let body = request_body(&mut socket)
            .await
            .expect("unsupported local provider request");
        let body: Value = serde_json::from_slice(&body).unwrap();
        let tools = body["tools"]
            .as_array()
            .expect("missing model tool inventory")
            .clone();
        let completed = json!({"type":"response.completed","response":{"id":"resp_baton_probe","status":"completed","output":[]}});
        let mut events = Vec::new();
        if flow != ProbeFlow::Inventory {
            let (name, arguments) = if flow == ProbeFlow::DynamicRefusal {
                ("baton_probe", json!({}))
            } else {
                (
                    "request_user_input",
                    json!({"questions":[{"id":"q","header":"Choice","question":"Synthetic choice?","options":[{"label":"A","description":"First"},{"label":"B","description":"Second"}]}]}),
                )
            };
            events.push(json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc_baton_probe","call_id":"call_baton_probe","name":name,"arguments":arguments.to_string(),"status":"completed"}}));
        }
        events.push(completed.clone());
        respond(&mut socket, &events).await;
        if flow == ProbeFlow::DynamicRefusal {
            let (mut socket, peer) = listener.accept().await.unwrap();
            assert!(peer.ip().is_loopback());
            let body = request_body(&mut socket).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert!(
                body.to_string().contains("BATON_EXECUTION_NOT_ENABLED"),
                "failure result was not forwarded"
            );
            respond(&mut socket, &[completed]).await;
        }
        tools
    });
    let config = format!(
        r#"model = "baton-fixture"
model_provider = "baton_mock"
approval_policy = "never"
sandbox_mode = "read-only"
web_search = "disabled"
[analytics]
enabled = false
[feedback]
enabled = false
[agents]
enabled = false
[apps._default]
enabled = false
[tools]
view_image = false
[features]
goals = false
view_image = false
shell_tool = false
shell_snapshot = false
remote_plugin = false
skill_mcp_dependency_install = false
[model_providers.baton_mock]
name = "Baton local test provider"
base_url = "http://{address}/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
stream_idle_timeout_ms = 5000
"#
    );
    std::fs::write(home.join("config.toml"), config).unwrap();
    let mut environment = vec![
        ("CODEX_HOME".into(), home.as_os_str().to_owned()),
        ("HOME".into(), home.as_os_str().to_owned()),
        ("USERPROFILE".into(), home.as_os_str().to_owned()),
    ];
    if cfg!(windows) {
        if let Some(root) = std::env::var_os("SystemRoot") {
            environment.push(("SystemRoot".into(), root));
        }
    }
    let spec = LaunchSpec::new(
        Path::new(&binary),
        &f.repo.path(),
        ["app-server".into(), "--listen".into(), "stdio://".into()],
    )
    .unwrap()
    .with_piped_stdio()
    .with_environment(environment)
    .unwrap();
    let mut session = f.launch_spec(spec);
    let (stdin, stdout) = session.take_stdio().unwrap();
    let mut writer = tokio::process::ChildStdin::from_std(stdin).unwrap();
    let mut reader = BufReader::new(tokio::process::ChildStdout::from_std(stdout).unwrap());
    let trace = async {
        send(&mut writer, json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"baton_inventory_probe","version":"0.1.0"},"capabilities":{"experimentalApi":true,"explicitGatewayOauth":true}}})).await;
        let reply = frame(&mut reader).await;
        assert_eq!(reply["id"], 1);
        assert!(reply.get("result").is_some(), "initialize refused");
        send(&mut writer, json!({"method":"initialized","params":{}})).await;
        send(&mut writer, json!({"id":2,"method":"thread/start","params":{
            "cwd":f.repo.path(),"model":"baton-fixture","modelProvider":"baton_mock","sandbox":"read-only","approvalPolicy":"never","ephemeral":true,
            "dynamicTools":[{"type":"function","name":"baton_probe","description":"Synthetic no-op; never execute commands.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}]
        }})).await;
        let thread = loop {
            let reply = frame(&mut reader).await;
            if reply["id"] == 2 {
                assert!(reply.get("error").is_none(), "thread/start refused");
                break reply["result"]["thread"]["id"]
                    .as_str()
                    .expect("missing thread ID")
                    .to_owned();
            }
        };
        let mut turn = json!({"id":3,"method":"turn/start","params":{"threadId":thread,"input":[{"type":"text","text":"Synthetic probe. Reply OK without calling tools."}]}});
        if flow == ProbeFlow::InputBlocked {
            turn["params"]["collaborationMode"] = json!({"mode":"plan","settings":{"model":"baton-fixture","reasoning_effort":null,"developer_instructions":null}});
        }
        send(&mut writer, turn).await;
        let mut callback_seen = false;
        loop {
            let message = frame(&mut reader).await;
            if message["method"] == "item/tool/call" {
                assert!(
                    flow == ProbeFlow::DynamicRefusal,
                    "unexpected dynamic request"
                );
                assert_eq!(message["params"]["threadId"], thread);
                assert_eq!(message["params"]["tool"], "baton_probe");
                assert_eq!(message["params"]["arguments"], json!({}));
                send(&mut writer, json!({"id":message["id"],"result":{"success":false,"contentItems":[{"type":"inputText","text":"BATON_EXECUTION_NOT_ENABLED"}]}})).await;
                callback_seen = true;
            }
            if message["method"] == "item/tool/requestUserInput" {
                assert!(flow == ProbeFlow::InputBlocked, "unexpected input request");
                assert_eq!(message["params"]["threadId"], thread);
                assert_eq!(message["params"]["questions"][0]["id"], "q");
                callback_seen = true;
                break;
            }
            if message["method"] == "turn/completed" {
                break;
            }
        }
        assert!(
            flow == ProbeFlow::Inventory || callback_seen,
            "native callback was not observed"
        );
        server.await.unwrap()
    };
    let tools = tokio::time::timeout(Duration::from_secs(30), trace).await;
    let run = f
        .store
        .run(&f.actor, f.task, session.binding().run_id)
        .unwrap();
    session
        .observe(
            &mut f.store,
            &f.actor,
            Id::new_v4(),
            RunObservation::AdapterGap {
                source_sequence: run.source_cursor + 1,
            },
        )
        .unwrap();
    session
        .stop(&mut f.store, &f.actor, Duration::from_secs(5))
        .unwrap();
    let tools = tools.expect("native inventory probe deadline exceeded");
    tools
        .iter()
        .map(|tool| {
            tool.get("name")
                .and_then(Value::as_str)
                .unwrap_or("<non-function>")
                .to_owned()
        })
        .collect()
}
