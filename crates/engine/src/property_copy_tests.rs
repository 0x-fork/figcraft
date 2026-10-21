//! Copying one fill, stroke or effect row (`fill.copy`, `stroke.copy`, `effect.copy`) and pasting
//! it onto other layers (`fill.paste`, `stroke.paste`, `effect.paste`): the Design panel's row
//! menus.

use figcraft_color::{Color, Paint};
use figcraft_doc::{Effect, EffectKind, NodeId};
use serde_json::{Value, json};

use crate::Session;

fn id(v: Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn rect(s: &mut Session) -> NodeId {
    id(ex(s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10})))
}

fn node(s: &Session, id: NodeId) -> figcraft_doc::Node {
    s.doc().unwrap().node(id).unwrap().clone()
}

#[test]
fn a_copied_fill_replaces_a_fill_row_or_adds_one() {
    let mut s = Session::new();
    let a = rect(&mut s);
    let b = rect(&mut s);
    let red = Paint::solid(Color::from_hex("#FF0000").unwrap());
    ex(&mut s, "fill.add", json!({"ids": [a], "paint": red}));
    let copied = ex(&mut s, "fill.copy", json!({"id": a, "index": 1}));
    assert_eq!(copied["type"], "solid", "the copy reports the paint it took");
    // Onto b's fill row: it replaces that paint.
    ex(&mut s, "fill.paste", json!({"ids": [b], "index": 0}));
    assert_eq!(node(&s, b).fills, vec![red.clone()]);
    // With no row: it adds one on top.
    ex(&mut s, "fill.paste", json!({"ids": [b]}));
    assert_eq!(node(&s, b).fills, [red.clone(), red]);
    // Each paste is one undo step.
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(node(&s, b).fills.len(), 1);
}

#[test]
fn a_copied_stroke_pastes_as_a_stroke_only() {
    let mut s = Session::new();
    let a = rect(&mut s);
    let b = rect(&mut s);
    let blue = Paint::solid(Color::from_hex("#0000FF").unwrap());
    ex(&mut s, "stroke.add", json!({"ids": [a], "paint": blue}));
    ex(&mut s, "node.set", json!({"ids": [a], "props": {"strokeWeight": 3.0, "strokeAlign": "outside"}}));
    ex(&mut s, "stroke.copy", json!({"id": a, "index": 0}));
    ex(&mut s, "stroke.add", json!({"ids": [b]}));
    ex(&mut s, "stroke.paste", json!({"ids": [b], "index": 0}));
    assert_eq!(node(&s, b).strokes, [blue]);
    assert_eq!(node(&s, b).stroke, node(&s, a).stroke, "the weight and alignment come with the paint");
    assert_eq!((node(&s, b).stroke.weight, node(&s, b).stroke.align), (3.0, figcraft_doc::StrokeAlign::Outside));
    // A copied fill doesn't paste as a stroke, and the target keeps its stroke.
    ex(&mut s, "fill.copy", json!({"id": a}));
    assert!(s.execute("stroke.paste", &json!({"ids": [b]})).is_err());
    assert_eq!(node(&s, b).strokes.len(), 1);
    // A layer with no stroke has nothing to copy.
    assert!(s.execute("stroke.copy", &json!({"id": b, "index": 1})).is_err());
}

#[test]
fn a_copied_effect_replaces_an_effect_row_or_adds_one() {
    let mut s = Session::new();
    let a = rect(&mut s);
    let b = rect(&mut s);
    ex(&mut s, "effect.add", json!({"ids": [a], "effect": {"type": "dropShadow", "radius": 7, "offset": [2, 3]}}));
    ex(&mut s, "effect.copy", json!({"id": a, "index": 0}));
    let e = node(&s, a).effects[0].clone();
    // Onto b's effect row: it replaces that effect.
    ex(&mut s, "effect.add", json!({"ids": [b], "type": "layerBlur"}));
    ex(&mut s, "effect.paste", json!({"ids": [b], "index": 0}));
    assert_eq!(node(&s, b).effects, vec![e.clone()]);
    // With no row: it adds one on top.
    ex(&mut s, "effect.paste", json!({"ids": [b]}));
    assert_eq!(node(&s, b).effects, [e.clone(), e]);
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(node(&s, b).effects.len(), 1);
    let Effect { kind: EffectKind::DropShadow(shadow), visible } = node(&s, b).effects[0].clone() else {
        panic!("the pasted effect is a drop shadow")
    };
    assert_eq!((shadow.radius, shadow.offset, visible), (7.0, (2.0, 3.0), true));
}
