//! `inspect_document` must not emit a message larger than a client can read in one piece: MCP
//! stdio is newline-delimited JSON, so one result is one message, and a client whose read buffer
//! it exceeds closes the pipe and kills the server for the rest of the session (#912).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::Client;
use serde_json::{Value, json};

/// A document whose tree is far larger than the limits used below.
fn busy() -> Client {
    let mut c = Client::new();
    for i in 0..40 {
        let frame = c.create(json!({"type": "frame", "x": i * 10, "y": 0, "width": 200, "height": 200}));
        for j in 0..12 {
            c.create(json!({"type": "rectangle", "parent": frame, "x": j * 5, "y": 5, "width": 20, "height": 20}));
        }
    }
    c
}

/// The response and what its text block takes on the wire, escaped as the transport sends it.
fn inspect(c: &mut Client, args: Value) -> (Value, usize) {
    let r = c.call("inspect_document", args.clone());
    assert_eq!(r["isError"], false, "{args}: {}", r["content"][0]["text"]);
    let text = r["content"][0]["text"].as_str().expect("a text block");
    let wire = serde_json::to_string(&text).unwrap().len();
    (serde_json::from_str(text).expect("the block is JSON"), wire)
}

#[test]
fn a_tree_too_large_for_the_client_is_reported_shallower_and_says_so() {
    let mut c = busy();
    let (full, full_wire) = inspect(&mut c, json!({"depth": 6, "allPages": true}));
    assert!(full["truncated"].is_null(), "a tree within the limit is not truncated: {}", full["truncated"]);
    assert!(full_wire > 30_000, "the fixture has to outgrow the limits below: {full_wire}");

    // Asked for more than the caller can read: a shallower tree, inside the limit, and a marker.
    let (cut, cut_wire) = inspect(&mut c, json!({"depth": 6, "allPages": true, "maxBytes": 30_000}));
    assert!(cut_wire <= 30_000, "the response still exceeds maxBytes: {cut_wire}");
    let t = &cut["truncated"];
    assert_eq!(t["requestedDepth"], 6, "{t}");
    assert!(t["depth"].as_u64().unwrap() < 6, "the depth was reduced: {t}");
    assert_eq!(t["maxBytes"], 30_000, "{t}");
    assert_eq!(t["requestedDepthBytes"], full_wire, "the marker says what the full tree would have taken: {t}");
    // The marker points at the way to read the rest.
    assert!(t["hint"].as_str().unwrap_or("").contains("get_node"), "{t}");
    // Still a usable document, not just a marker: the page and its top-level layers are there.
    assert!(!cut["currentPage"].is_null(), "{cut}");
    assert!(cut["pages"][0]["children"].as_array().is_some_and(|c| !c.is_empty()), "{cut}");

    // The deepest tree that fits is the one sent: a limit that admits the whole tree is not cut.
    let (whole, _) = inspect(&mut c, json!({"depth": 6, "allPages": true, "maxBytes": full_wire}));
    assert!(whole["truncated"].is_null(), "a limit at the exact size is not a truncation: {}", whole["truncated"]);
}

#[test]
fn a_limit_below_the_shallowest_tree_still_answers() {
    // Nothing smaller than depth 0 exists, so it is sent with the marker rather than an error:
    // an error would leave the caller with nothing at all.
    let mut c = busy();
    let (v, _) = inspect(&mut c, json!({"depth": 6, "allPages": true, "maxBytes": 1024}));
    assert_eq!(v["truncated"]["depth"], 0, "{}", v["truncated"]);
    assert_eq!(v["truncated"]["requestedDepth"], 6, "{}", v["truncated"]);
}

#[test]
fn maxbytes_is_checked() {
    let mut c = Client::new();
    let r = c.call("inspect_document", json!({"depth": 1, "maxBytes": 10}));
    assert_eq!(r["isError"], true, "{r}");
    assert!(r["content"][0]["text"].as_str().unwrap_or("").contains("at least 1024"), "{r}");
}
