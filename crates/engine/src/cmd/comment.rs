//! Comments: add, reply, edit, resolve, delete and list comment threads.

use figcraft_doc::*;
use serde_json::{Value, json};

use super::{CommandSpec, ok};
use crate::params::*;
use crate::{Session, bad, spec};

const DEFAULT_AUTHOR: &str = "You";

pub fn now_secs() -> u64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn req_comment(cmd: &str, s: &Session, p: &Value) -> crate::Result<u64> {
    let id = req_num(cmd, p, "id")? as u64;
    if s.doc()?.comment(id).is_none() {
        return Err(bad(cmd, format!("no comment {id}")));
    }
    Ok(id)
}

fn author(p: &Value) -> String {
    string(p, "author").map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).unwrap_or_else(|| DEFAULT_AUTHOR.into())
}

fn text(cmd: &str, p: &Value) -> crate::Result<String> {
    let t = req_string(cmd, p, "text")?;
    if t.trim().is_empty() {
        return Err(bad(cmd, "comment text is empty"));
    }
    Ok(t)
}

/// JSON view of a comment (with its resolved page position).
pub fn comment_json(doc: &Document, c: &Comment) -> Value {
    let (x, y) = c.position(doc);
    json!({
        "id": c.id, "page": c.page, "x": x, "y": y, "anchor": c.anchor,
        "author": c.author, "text": c.text, "created": c.created, "resolved": c.resolved,
        "replies": c.replies.iter().map(|r| json!({"author": r.author, "text": r.text, "created": r.created})).collect::<Vec<_>>(),
    })
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "comment.add",
            "Add comment",
            "{x, y, text, page?, author?, anchor? (node id to pin to; true = topmost frame under the point)}",
            true,
            |s, p| {
                let x = req_num("comment.add", p, "x")?;
                let y = req_num("comment.add", p, "y")?;
                let text = text("comment.add", p)?;
                let page = id(p, "page").unwrap_or(s.page()?);
                let doc = s.doc()?;
                if !doc.pages.contains(&page) {
                    return Err(bad("comment.add", format!("{page} is not a page")));
                }
                // Anchor: an explicit node, or (anchor: true) the innermost frame under the point.
                let anchor = match get(p, "anchor") {
                    Some(Value::Bool(true)) => frame_at(doc, page, x, y),
                    Some(v) => id_of(v).filter(|a| doc.node(*a).is_some()),
                    None => None,
                };
                let (lx, ly) = match anchor {
                    Some(a) => {
                        let l = doc.world_transform(a).inverse() * geom::Point::new(x, y);
                        (l.x, l.y)
                    }
                    None => (x, y),
                };
                let c = Comment {
                    id: doc.next_comment_id(),
                    page,
                    x: lx,
                    y: ly,
                    anchor,
                    author: author(p),
                    text,
                    created: now_secs(),
                    resolved: false,
                    replies: Vec::new(),
                };
                let id = c.id;
                s.doc_mut()?.comments.push(c);
                Ok(json!({"id": id}))
            }
        ),
        spec!("comment.reply", "Reply to comment", "{id, text, author?}", true, |s, p| {
            let id = req_comment("comment.reply", s, p)?;
            let r = Reply { author: author(p), text: text("comment.reply", p)?, created: now_secs() };
            if let Some(c) = s.doc_mut()?.comment_mut(id) {
                c.replies.push(r);
                c.resolved = false;
            }
            ok()
        }),
        spec!("comment.edit", "Edit comment", "{id, text, reply? (index; omit for the first message)}", true, |s, p| {
            let id = req_comment("comment.edit", s, p)?;
            let t = text("comment.edit", p)?;
            let reply = num(p, "reply").map(|r| r as usize);
            let c = s.doc_mut()?.comment_mut(id).ok_or_else(|| bad("comment.edit", "no comment"))?;
            match reply {
                Some(i) => c.replies.get_mut(i).ok_or_else(|| bad("comment.edit", format!("no reply {i}")))?.text = t,
                None => c.text = t,
            }
            ok()
        }),
        spec!("comment.resolve", "Resolve comment", "{id, resolved? (default true)}", true, |s, p| {
            let id = req_comment("comment.resolve", s, p)?;
            let v = boolean(p, "resolved").unwrap_or(true);
            if let Some(c) = s.doc_mut()?.comment_mut(id) {
                c.resolved = v;
            }
            ok()
        }),
        spec!("comment.move", "Move comment", "{id, x, y}", true, |s, p| {
            let id = req_comment("comment.move", s, p)?;
            let (x, y) = (req_num("comment.move", p, "x")?, req_num("comment.move", p, "y")?);
            let doc = s.doc_mut()?;
            let anchor = doc.comment(id).and_then(|c| c.anchor).filter(|a| doc.node(*a).is_some());
            let (lx, ly) = match anchor {
                Some(a) => {
                    let l = doc.world_transform(a).inverse() * geom::Point::new(x, y);
                    (l.x, l.y)
                }
                None => (x, y),
            };
            if let Some(c) = doc.comment_mut(id) {
                c.x = lx;
                c.y = ly;
                if anchor.is_none() {
                    c.anchor = None;
                }
            }
            ok()
        }),
        spec!("comment.delete", "Delete comment", "{id, reply? (index: delete one reply)}", true, |s, p| {
            let id = req_comment("comment.delete", s, p)?;
            let reply = num(p, "reply").map(|r| r as usize);
            let doc = s.doc_mut()?;
            match reply {
                Some(i) => {
                    let c = doc.comment_mut(id).ok_or_else(|| bad("comment.delete", "no comment"))?;
                    if i >= c.replies.len() {
                        return Err(bad("comment.delete", format!("no reply {i}")));
                    }
                    c.replies.remove(i);
                }
                None => doc.comments.retain(|c| c.id != id),
            }
            ok()
        }),
        spec!(
            "comment.list",
            "List comments",
            "{page? (default: current; \"all\" for every page), resolved? (include resolved, default true)}",
            false,
            |s, p| {
                let doc = s.doc()?;
                let all = string(p, "page").is_some_and(|v| v == "all");
                let page = if all { None } else { Some(id(p, "page").unwrap_or(s.page()?)) };
                let include_resolved = boolean(p, "resolved").unwrap_or(true);
                let list: Vec<Value> = doc
                    .comments
                    .iter()
                    .filter(|c| doc.pages.contains(&c.page))
                    .filter(|c| page.is_none_or(|pg| c.page == pg))
                    .filter(|c| include_resolved || !c.resolved)
                    .map(|c| comment_json(doc, c))
                    .collect();
                Ok(json!({"comments": list}))
            }
        ),
    ]
}

/// Innermost frame (or component/instance/section) under a page point, for anchoring.
fn frame_at(doc: &Document, page: NodeId, x: f64, y: f64) -> Option<NodeId> {
    let pt = geom::Point::new(x, y);
    let mut found = None;
    let mut stack = vec![page];
    while let Some(parent) = stack.pop() {
        let Some(n) = doc.node(parent) else { continue };
        for c in n.children.iter().rev() {
            let Some(cn) = doc.node(*c) else { continue };
            if !cn.visible || !cn.kind.is_container() {
                continue;
            }
            let l = doc.world_transform(*c).inverse() * pt;
            if l.x >= 0.0 && l.y >= 0.0 && l.x <= cn.width && l.y <= cn.height {
                if cn.kind.is_frame_like() || matches!(cn.kind, NodeKind::Section { .. }) {
                    found = Some(*c);
                }
                stack.push(*c);
                break;
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn comment_thread_lifecycle_and_anchoring() {
        let mut s = Session::new();
        let f = s.execute("node.create", &json!({"type": "frame", "x": 100, "y": 100, "width": 200, "height": 200})).unwrap();
        let fid = crate::params::id_of(&f["id"]).unwrap();
        let c = s.execute("comment.add", &json!({"x": 150, "y": 160, "text": "Tighten this", "anchor": true})).unwrap();
        let cid = c["id"].as_u64().unwrap();
        assert_eq!(s.doc().unwrap().comment(cid).unwrap().anchor, Some(fid));
        // The pin follows its frame.
        s.execute("node.set", &json!({"id": fid, "props": {"x": 300}})).unwrap();
        let l = s.execute("comment.list", &json!({})).unwrap();
        assert_eq!(l["comments"][0]["x"].as_f64().unwrap(), 350.0);
        s.execute("comment.reply", &json!({"id": cid, "text": "Done", "author": "Sam"})).unwrap();
        s.execute("comment.resolve", &json!({"id": cid})).unwrap();
        let open = s.execute("comment.list", &json!({"resolved": false})).unwrap();
        assert!(open["comments"].as_array().unwrap().is_empty());
        assert_eq!(s.doc().unwrap().comment(cid).unwrap().participants(), vec!["You", "Sam"]);
        assert!(s.execute("comment.add", &json!({"x": 0, "y": 0, "text": "  "})).is_err());
        assert!(s.execute("comment.reply", &json!({"id": 999, "text": "x"})).is_err());
        s.execute("comment.delete", &json!({"id": cid})).unwrap();
        assert!(s.doc().unwrap().comments.is_empty());
        assert!(s.undo().unwrap());
        assert_eq!(s.doc().unwrap().comments.len(), 1);
    }
}
