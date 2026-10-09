//! Localhost JSON-lines control server: each line `{"id", "method", "params"}` is queued for the UI
//! thread; the reply line `{"id", "ok", "result" | "error"}` is written when the frame handled it.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use figcraft_ui_egui::ControlRequest;
use serde_json::{Value, json};

type Queue = Arc<Mutex<Vec<ControlRequest>>>;
type CtxSlot = Arc<Mutex<Option<egui::Context>>>;

pub fn start(port: u16, queue: Queue, ctx: CtxSlot) {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            log::error!("control server: can't bind 127.0.0.1:{port}: {e} (another app may own it; pick another port)");
            eprintln!("FigCraft control server failed to bind port {port}: {e}");
            return;
        }
    };
    eprintln!("FigCraft control server listening on 127.0.0.1:{port}");
    let spawned = std::thread::Builder::new().name("figcraft-control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let (q, c) = (queue.clone(), ctx.clone());
            let _ = std::thread::Builder::new().name("figcraft-control-conn".into()).spawn(move || serve(stream, q, c));
        }
    });
    if let Err(e) = spawned {
        log::error!("control server thread: {e}");
    }
}

fn serve(stream: TcpStream, queue: Queue, ctx: CtxSlot) {
    let Ok(read_half) = stream.try_clone() else { return };
    let mut out = stream;
    let reader = BufReader::new(read_half);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(v) => {
                let id = v.get("id").cloned().unwrap_or(Value::Null);
                let method = v.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                let params = v.get("params").cloned().unwrap_or(json!({}));
                let (tx, rx) = std::sync::mpsc::channel();
                queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(ControlRequest { id: id.clone(), method, params, reply: tx });
                if let Some(c) = ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
                    c.request_repaint();
                }
                match rx.recv_timeout(Duration::from_secs(60)) {
                    Ok(r) => match r.result {
                        Ok(res) => json!({"id": r.id, "ok": true, "result": res}),
                        Err(e) => json!({"id": r.id, "ok": false, "error": e}),
                    },
                    Err(_) => json!({"id": id, "ok": false, "error": "timed out waiting for the UI (is the window visible?)"}),
                }
            }
            Err(e) => json!({"id": null, "ok": false, "error": format!("bad JSON: {e}")}),
        };
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}
