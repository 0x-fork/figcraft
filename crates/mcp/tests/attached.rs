//! The attached backend against a fake app: a loopback control server answering with a headless
//! session, speaking the desktop app's JSON-lines protocol.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use figcraft_mcp::{Attached, Backend, Headless, Server};
use serde_json::{Value, json};

fn fake_app() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut h = Headless::new();
        for stream in listener.incoming().flatten() {
            let mut out = stream.try_clone().unwrap();
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                let method = v["method"].as_str().unwrap_or("");
                let reply = match h.call(method, v["params"].clone()) {
                    Ok(r) => json!({"id": v["id"], "ok": true, "result": r}),
                    Err(e) => json!({"id": v["id"], "ok": false, "error": e}),
                };
                if writeln!(out, "{reply}").is_err() {
                    break;
                }
            }
        }
    });
    port
}

fn call(s: &mut Server, tool: &str, args: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": args}}).to_string();
    let r: Value = serde_json::from_str(&s.handle_line(&line).unwrap()).unwrap();
    r["result"].clone()
}

#[test]
fn attached_backend_forwards_everything() {
    let port = fake_app();
    let backend = Attached::port(port).unwrap();
    assert!(backend.describe().contains(&port.to_string()));
    let mut s = Server::new(Box::new(backend));
    let r = call(&mut s, "create_node", json!({"type": "frame", "width": 64, "height": 32, "props": {"fill": "#FF0000"}}));
    assert_eq!(r["isError"], false, "{r}");
    let created: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
    let id = created["id"].clone();
    // Render through ui.render + a temp file.
    let shot = call(&mut s, "screenshot", json!({}));
    assert_eq!(shot["content"][0]["type"], "image", "{shot}");
    let shot = call(&mut s, "screenshot", json!({"ids": [id], "scale": 2}));
    assert_eq!(shot["isError"], false, "{shot}");
    // Window capture through ui.screenshot.
    let win = call(&mut s, "screenshot", json!({"window": true}));
    assert_eq!(win["content"][0]["type"], "image", "{win}");
    let e = call(&mut s, "export", json!({"ids": [id]}));
    assert_eq!(e["content"][0]["mimeType"], "image/png");
    assert!(call(&mut s, "key", json!({"key": "Delete"}))["isError"] == false);
    let doc = call(&mut s, "inspect_document", json!({}));
    assert!(!doc["content"][0]["text"].as_str().unwrap().contains("\"Frame"));
    let err = call(&mut s, "run_command", json!({"command": "nope"}));
    assert_eq!(err["isError"], true);
}

#[test]
fn attached_to_nothing_fails_cleanly() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(Attached::port(port).is_err());
}
