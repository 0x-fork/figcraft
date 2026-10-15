//! `node.set` refuses what can't apply instead of storing it where nothing reads it: auto layout
//! keys on layers that can't have auto layout, and a `fontStyle` the font family doesn't have.

use serde_json::{Value, json};

use crate::{EngineError, Session};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn id(v: &Value) -> crate::NodeId {
    crate::params::id_of(v).unwrap()
}

fn bad_msg(r: crate::Result<Value>) -> String {
    match r {
        Err(EngineError::BadParams { msg, .. }) => msg,
        other => panic!("expected BadParams, got {other:?}"),
    }
}

#[test]
fn auto_layout_keys_on_layers_without_auto_layout_are_refused() {
    let mut s = Session::new();
    let t = id(&ex(&mut s, "node.create", json!({"type": "text", "text": "Hi"}))["id"]);
    let r = id(&ex(&mut s, "node.create", json!({"type": "rectangle"}))["id"]);
    for (target, key, value) in [(t, "gap", json!(8)), (r, "padding", json!(4)), (t, "mainAlign", json!("center")), (r, "wrap", json!(true))] {
        let msg = bad_msg(s.execute("node.set", &json!({"ids": [target], "props": {key: value}})));
        assert!(msg.contains(key) && msg.contains("auto layout"), "{msg}");
        assert!(s.doc().unwrap().node(target).unwrap().layout.is_none(), "no hidden auto layout on {target}");
    }
    // Frames still take them, with or without a layout mode first.
    let f = id(&ex(&mut s, "node.create", json!({"type": "frame"}))["id"]);
    ex(&mut s, "node.set", json!({"ids": [f], "props": {"gap": 8, "padding": 4}}));
    assert_eq!(s.doc().unwrap().node(f).unwrap().layout.as_ref().map(|l| l.gap), Some(8.0));
}

#[test]
fn a_font_style_the_family_lacks_is_refused_with_the_styles_it_has() {
    figcraft_text::scan_system_fonts();
    // Any installed family with a known list of styles will do (CI machines differ).
    let Some((family, styles)) =
        figcraft_text::family_names().into_iter().map(|f| (f.clone(), figcraft_text::family_styles(&f))).find(|(_, st)| !st.is_empty())
    else {
        return;
    };
    let mut s = Session::new();
    let t = id(&ex(&mut s, "node.create", json!({"type": "text", "text": "Hi", "props": {"fontFamily": family}}))["id"]);
    let msg = bad_msg(s.execute("node.set", &json!({"ids": [t], "props": {"fontStyle": "Definitely Not A Style"}})));
    assert!(msg.contains(&family) && msg.contains(&styles[0]), "{msg}");
    ex(&mut s, "node.set", json!({"ids": [t], "props": {"fontStyle": styles[0]}}));
    // A family that isn't installed keeps any style name (files from elsewhere name theirs).
    ex(&mut s, "node.set", json!({"ids": [t], "props": {"fontFamily": "No Such Family 7f3a", "fontStyle": "Hairline"}}));
}
