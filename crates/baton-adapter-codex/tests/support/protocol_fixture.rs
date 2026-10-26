//! Synthetic local protocol peer. It never calls a model or executes requested tools.
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::{BufRead, Write},
    path::Path,
    time::Duration,
};
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|mode| mode == "startup-canary") {
        assert_eq!(args.len(), 3);
        let mut marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])
            .unwrap();
        marker.write_all(b"started").unwrap();
        marker.sync_all().unwrap();
        return;
    }
    let root = Path::new(&args[2]);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let _alive = options.open(root.join("alive")).unwrap();
    let mut reader = std::io::stdin().lock();
    let mut request = String::new();
    reader.read_line(&mut request).unwrap();
    let request: Value = serde_json::from_str(&request).unwrap();
    assert_eq!(request["method"], "initialize");
    let mode = args[1].to_str().unwrap();
    assert_eq!(
        request["params"]["capabilities"]["experimentalApi"],
        mode.starts_with("register-") && mode != "register-disabled"
    );
    assert_eq!(
        request["params"]["capabilities"]["explicitGatewayOauth"],
        true
    );
    std::fs::write(root.join("received"), b"initialize").unwrap();
    if mode == "eof" {
        return;
    }
    if mode == "hang" {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let mut reply = json!({"id":1,"result":{
        "userAgent":"synthetic/0.163.0", "codexHome":"/synthetic/private",
        "platformFamily":"windows","platformOs":"windows"
    }});
    if mode == "wrong-id" {
        reply["id"] = json!(2);
    }
    let bytes = match mode {
        "oversized" => format!("{}\n", "x".repeat(65_536)),
        "partial" => "{\"id\":1".into(),
        _ => format!("{reply}\n"),
    };
    std::io::stdout().write_all(bytes.as_bytes()).unwrap();
    std::io::stdout().flush().unwrap();
    if mode == "partial" {
        return;
    }
    let mut ack = String::new();
    if reader.read_line(&mut ack).unwrap() > 0 {
        assert_eq!(
            serde_json::from_str::<Value>(&ack).unwrap(),
            json!({"method":"initialized","params":{}})
        );
        std::fs::write(root.join("acknowledged"), b"initialized").unwrap();
        if mode.starts_with("register-") {
            let mut request = String::new();
            if reader.read_line(&mut request).unwrap() == 0 {
                return;
            }
            let mut request: Value = serde_json::from_str(&request).unwrap();
            if request["method"] == "config/read" {
                assert_eq!(request["params"]["includeLayers"], false);
                let requested: std::path::PathBuf =
                    serde_json::from_value(request["params"]["cwd"].clone()).unwrap();
                assert_eq!(
                    std::fs::canonicalize(requested).unwrap(),
                    std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap()
                );
                std::fs::write(root.join("config-received"), b"config/read").unwrap();
                let mut response = json!({"id":request["id"],"result":{"config":{"mcp_servers":{},"hooks":{},"plugins":{},"marketplaces":{},"notify":null},"origins":{},"layers":null}});
                let field = mode.strip_prefix("register-config-");
                if let Some(field) = field {
                    if field == "mcp" {
                        response["result"]["config"]["mcp_servers"] =
                            json!({"private-service":{"enabled":false}});
                    } else if field == "notify" {
                        response["result"]["config"][field] = json!(["private-service"]);
                    } else {
                        response["result"]["config"][field] = json!({"private-service":{}});
                    }
                }
                match mode {
                    "register-config-rejected" => {
                        response = json!({"id":request["id"],"error":{"code":-1,"message":"private configuration rejection"}})
                    }
                    "register-config-wrong-id" => response["id"] = json!("unrelated"),
                    "register-config-mixed" => response["error"] = json!(null),
                    "register-config-malformed" => {
                        response["result"]["config"]["mcp_servers"] = json!([])
                    }
                    "register-config-missing" => {
                        response["result"].as_object_mut().unwrap().remove("config");
                    }
                    "register-config-hang" => {
                        std::thread::sleep(Duration::from_secs(30));
                        return;
                    }
                    _ => (),
                }
                println!("{response}");
                std::io::stdout().flush().unwrap();
                let mut next = String::new();
                if reader.read_line(&mut next).unwrap() == 0 {
                    return;
                }
                request = serde_json::from_str(&next).unwrap();
            }
            assert_eq!(request["method"], "thread/start");
            assert_eq!(request["params"]["ephemeral"], true);
            assert_eq!(request["params"]["sandbox"], "read-only");
            assert_eq!(request["params"]["approvalPolicy"], "never");
            assert_eq!(
                request["params"]["dynamicTools"][0]["name"],
                "baton_create_file_v1"
            );
            assert_eq!(
                request["params"]["dynamicTools"].as_array().unwrap().len(),
                1
            );
            std::fs::write(root.join("registration-received"), b"thread/start").unwrap();
            let mut response = json!({"id":request["id"],"result":{
                "thread":{"id":"thread-registered","ephemeral":true,"turns":[]},
                "cwd":std::env::current_dir().unwrap(),"approvalPolicy":"never",
                "sandbox":{"type":"readOnly","networkAccess":false},"model":"baton-fixture","modelProvider":"baton_mock"
            }});
            if mode == "register-wrong-id" {
                response["id"] = json!("unrelated");
            }
            if mode == "register-reject" {
                response = json!({"id":request["id"],"error":{"code":-1,"message":"private vendor rejection"}});
            }
            if mode == "register-unsafe" {
                response["result"]["sandbox"]["type"] = json!("dangerFullAccess");
            }
            if mode == "register-hang" {
                std::thread::sleep(Duration::from_secs(30));
                return;
            }
            let announced = json!({"method":"thread/started","emittedAtMs":1,"params":{"thread":{"id":"thread-registered","preview":"private preview"}}});
            if mode == "register-before" {
                println!(
                    "{}",
                    json!({"method":"remoteControl/status/changed","emittedAtMs":1,"params":{"status":"disabled","installationId":"private-id","serverName":"private-name","environmentId":null}})
                );
                println!("{announced}");
            }
            println!("{response}");
            if mode == "register-after" {
                println!("{announced}");
            }
            if mode == "register-create" {
                println!(
                    "{}",
                    json!({"method":"turn/started","params":{"threadId":"thread-registered","turn":{"id":"turn-1","status":"inProgress"}}})
                );
                println!(
                    "{}",
                    json!({"method":"item/started","params":{"threadId":"thread-registered","turnId":"turn-1","startedAtMs":1,"item":{"type":"dynamicToolCall","id":"call-1","tool":"baton_create_file_v1","status":"inProgress"}}})
                );
                println!(
                    "{}",
                    json!({"id":7,"method":"item/tool/call","params":{"threadId":"thread-registered","turnId":"turn-1","callId":"call-1","tool":"baton_create_file_v1","arguments":{"name":"created.txt","content":"must stay blocked"}}})
                );
            }
            std::io::stdout().flush().unwrap();
            let mut replay = String::new();
            if reader.read_line(&mut replay).unwrap() > 0 {
                std::fs::write(root.join("registration-replayed"), b"unexpected request").unwrap();
            }
            return;
        }
        if mode.starts_with("create-") {
            println!(
                "{}",
                json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}})
            );
            println!(
                "{}",
                json!({"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","startedAtMs":1,"item":{"type":"dynamicToolCall","id":"call-1","tool":"baton_create_file_v1","status":"inProgress"}}})
            );
            let callback = json!({"id":7,"method":"item/tool/call","params":{"threadId":"thread-1","turnId":"turn-1","callId":"call-1","tool":"baton_create_file_v1","arguments":{"name":"created.txt","content":"private created content"}}});
            println!("{callback}");
            std::io::stdout().flush().unwrap();
            let mut response = String::new();
            if reader.read_line(&mut response).unwrap() == 0 {
                return;
            }
            let response: Value = serde_json::from_str(&response).unwrap();
            if mode == "create-disabled" {
                assert_eq!(
                    response,
                    json!({"id":7,"result":{"success":false,"contentItems":[{"type":"inputText","text":"BATON_EXECUTION_NOT_ENABLED"}]}})
                );
                std::fs::write(root.join("creation-refused"), b"false").unwrap();
                return;
            }
            let digest = blake3::hash(b"private created content")
                .to_hex()
                .to_string();
            assert_eq!(
                response,
                json!({"id":7,"result":{"success":true,"contentItems":[{"type":"inputText","text":format!("BLAKE3:{digest}")}]}})
            );
            assert_eq!(
                std::fs::read("created.txt").unwrap(),
                b"private created content"
            );
            std::fs::write(root.join("creation-acknowledged"), b"true").unwrap();
            if mode == "create-duplicate" {
                println!("{callback}");
            } else if mode == "create-changed" {
                let mut changed = callback.clone();
                changed["params"]["arguments"]["content"] = json!("changed content");
                println!("{changed}");
            } else {
                println!(
                    "{}",
                    json!({"method":"serverRequest/resolved","params":{"threadId":"thread-1","requestId":7}})
                );
                println!(
                    "{}",
                    json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","completedAtMs":2,"item":{"type":"dynamicToolCall","id":"call-1","tool":"baton_create_file_v1","status":"completed","success":true}}})
                );
                println!(
                    "{}",
                    json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}})
                );
                if mode == "create-reuse" {
                    println!("{callback}");
                }
            }
            std::io::stdout().flush().unwrap();
        }
        if mode.starts_with("callback-") {
            println!(
                "{}",
                json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}})
            );
            let input = mode == "callback-input";
            let mut callback = if input {
                json!({"id":7,"method":"item/tool/requestUserInput","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"call-1","isBlocking":true,"questions":[{"id":"q","header":"Choice","question":"private question"}]}})
            } else {
                println!(
                    "{}",
                    json!({"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","startedAtMs":1,"item":{"type":"dynamicToolCall","id":"call-1","tool":"baton_probe","status":"inProgress","arguments":{"private":"private arguments"}}}})
                );
                json!({"id":7,"method":"item/tool/call","params":{"threadId":"thread-1","turnId":"turn-1","callId":"call-1","tool":"baton_probe","arguments":{"private":"private arguments"}}})
            };
            println!("{callback}");
            std::io::stdout().flush().unwrap();
            let mut response = String::new();
            if reader.read_line(&mut response).unwrap() == 0 {
                return;
            }
            if input {
                std::fs::write(root.join("invented-answer"), b"unexpected response").unwrap();
                panic!("input cannot be answered by a refusal policy");
            }
            let response: Value = serde_json::from_str(&response).unwrap();
            assert_eq!(
                response,
                json!({"id":7,"result":{"success":false,"contentItems":[{"type":"inputText","text":"BATON_EXECUTION_NOT_ENABLED"}]}})
            );
            std::fs::write(root.join("callback-refused"), b"false").unwrap();
            if mode == "callback-changed" {
                callback["params"]["arguments"] = json!({"private":"different arguments"});
                println!("{callback}");
            } else {
                println!(
                    "{}",
                    json!({"method":"serverRequest/resolved","params":{"threadId":"thread-1","requestId":7}})
                );
                println!(
                    "{}",
                    json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","completedAtMs":2,"item":{"type":"dynamicToolCall","id":"call-1","tool":"baton_probe","arguments":{},"status":"failed","success":false}}})
                );
                println!(
                    "{}",
                    json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"failed"}}})
                );
                if mode == "callback-reuse" {
                    println!("{callback}");
                }
            }
            std::io::stdout().flush().unwrap();
        }
        if mode.starts_with("interrupt-") {
            println!(
                "{}",
                json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}})
            );
            println!(
                "{}",
                json!({"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","startedAtMs":1,"item":{"type":"commandExecution","id":"item-1","status":"inProgress"}}})
            );
            std::io::stdout().flush().unwrap();
            let mut request = String::new();
            if reader.read_line(&mut request).unwrap() == 0 {
                return;
            }
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "turn/interrupt");
            assert_eq!(
                request["params"],
                json!({"threadId":"thread-1","turnId":"turn-1"})
            );
            std::fs::write(root.join("interrupt-received"), b"interrupt").unwrap();
            let mut reply = json!({"id": request["id"], "result":{}});
            if mode == "interrupt-wrong-id" {
                reply["id"] = json!("unrelated");
            }
            if mode == "interrupt-rejected" {
                reply =
                    json!({"id":request["id"],"error":{"code":-1,"message":"private rejection"}});
            }
            if mode == "interrupt-no-reply" {
                std::thread::sleep(Duration::from_secs(30));
                return;
            }
            let tool = json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","completedAtMs":2,"item":{"type":"commandExecution","id":"item-1","status":"declined","exitCode":null}}});
            let turn = json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"interrupted"}}});
            if mode == "interrupt-completed-first" {
                println!("{tool}\n{turn}\n{reply}");
            } else {
                println!("{reply}\n{tool}\n{turn}");
            }
            std::io::stdout().flush().unwrap();
        }
        if mode.starts_with("approval-") {
            let file = mode == "approval-file";
            let item_type = if file {
                "fileChange"
            } else {
                "commandExecution"
            };
            let method = if file {
                "item/fileChange/requestApproval"
            } else {
                "item/commandExecution/requestApproval"
            };
            let request_id = if mode == "approval-text" {
                json!("approval-1")
            } else {
                json!(7)
            };
            println!(
                "{}",
                json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}})
            );
            println!(
                "{}",
                json!({"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","startedAtMs":1,
                "item":{"type":item_type,"id":"item-1","status":"inProgress"}}})
            );
            let mut approval = json!({"id":request_id,"method":method,"params":{"threadId":"thread-1","turnId":"turn-1","itemId":"item-1",
                "startedAtMs":1,"command":"private command","reason":"private reason"}});
            if mode == "approval-no-cancel" {
                approval["params"]["availableDecisions"] = json!(["accept"]);
            }
            if mode == "approval-unsupported" {
                approval["method"] = json!("item/permissions/requestApproval");
            }
            println!("{approval}");
            std::io::stdout().flush().unwrap();
            let mut response = String::new();
            if reader.read_line(&mut response).unwrap() == 0 {
                return;
            }
            let response: Value = serde_json::from_str(&response).unwrap();
            if response["result"]["decision"] != "cancel" {
                std::fs::write(
                    root.join("dangerous-work"),
                    b"unexpected execution approval",
                )
                .unwrap();
                panic!("unsafe approval response");
            }
            assert_eq!(
                response,
                json!({"id":request_id,"result":{"decision":"cancel"}})
            );
            std::fs::write(root.join("cancel-reply"), b"cancel").unwrap();
            println!(
                "{}",
                json!({"method":"serverRequest/resolved","params":{"threadId":"thread-1","requestId":request_id}})
            );
            println!(
                "{}",
                json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","completedAtMs":2,
                "item":{"type":item_type,"id":"item-1","status":"declined","exitCode":null}}})
            );
            println!(
                "{}",
                json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"interrupted"}}})
            );
            if mode == "approval-reuse" {
                println!("{approval}");
            }
            std::io::stdout().flush().unwrap();
        }
        if mode == "passive" {
            println!(
                "{}",
                json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}})
            );
            for (index, kind) in [
                "userMessage",
                "agentMessage",
                "reasoning",
                "plan",
                "contextCompaction",
            ]
            .into_iter()
            .enumerate()
            {
                let item = json!({"id":format!("passive-{index}"),"type":kind,"text":"private text","content":["private content"],"summary":["private reasoning"]});
                println!(
                    "{}",
                    json!({"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","startedAtMs":1,"item":item}})
                );
                println!(
                    "{}",
                    json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","completedAtMs":2,"item":item}})
                );
            }
            println!(
                "{}",
                json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}})
            );
            std::io::stdout().flush().unwrap();
        }
        if mode == "trace"
            || mode == "orphan"
            || mode == "foreign-thread"
            || mode == "changed-duplicate"
        {
            let start = json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress","items":[]}}});
            let tool = json!({"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","startedAtMs":1,
                "item":{"type":"commandExecution","id":"item-1","status":"inProgress","command":"private command"}}});
            let complete = json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","completedAtMs":2,
                "item":{"type":"commandExecution","id":"item-1","status":"completed","exitCode":7,"aggregatedOutput":"private output"}}});
            let turn = json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"failed"}}});
            let events = match mode {
                "trace" => vec![start, tool.clone(), tool, complete, turn],
                "orphan" => vec![complete],
                "foreign-thread" => {
                    let mut foreign = start;
                    foreign["params"]["threadId"] = json!("foreign-thread");
                    vec![foreign]
                }
                _ => {
                    let mut changed = tool.clone();
                    changed["params"]["item"]["type"] = json!("fileChange");
                    vec![start, tool, changed]
                }
            };
            for event in events {
                println!("{event}");
            }
            std::io::stdout().flush().unwrap();
        }
        let mut extra = String::new();
        assert_eq!(
            reader.read_line(&mut extra).unwrap(),
            0,
            "unexpected work request"
        );
    }
}
