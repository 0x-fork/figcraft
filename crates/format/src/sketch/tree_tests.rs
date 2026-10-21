//! Sketch layers → document layers: kinds, positions, transforms, constraints, visibility.

use figcraft_doc::color::PaintKind;
use figcraft_doc::{Document, HConstraint, NodeKind, VConstraint};
use serde_json::{Value, json};

use super::fixture::*;
use super::import;

/// Import one page holding `layers`, with extra archive `files`.
fn open_with(layers: Vec<Value>, files: &[(&str, &[u8])]) -> (Document, Vec<String>) {
    let z = sketch_zip("50.2", 103, document(&["P"]), &[page("P", "Page", layers)], files);
    import(&z, "f").unwrap()
}

fn open(layers: Vec<Value>) -> (Document, Vec<String>) {
    open_with(layers, &[])
}

fn node<'d>(doc: &'d Document, name: &str) -> &'d figcraft_doc::Node {
    doc.nodes().find(|n| n.name == name).unwrap_or_else(|| panic!("no layer named {name}"))
}

fn solid(doc: &Document, name: &str) -> Option<[f32; 4]> {
    node(doc, name).fills.iter().find_map(|p| match &p.kind {
        PaintKind::Solid { color } => Some([color.r, color.g, color.b, p.opacity]),
        _ => None,
    })
}

#[test]
fn artboard_is_frame_with_white_fill_unless_background() {
    let plain = artboard("A", "Plain", 0.0, 0.0, 100.0, 50.0, vec![]);
    let red = with(artboard("B", "Red", 200.0, 0.0, 100.0, 50.0, vec![]), "hasBackgroundColor", json!(true));
    let red = with(red, "backgroundColor", json!({"_class": "color", "red": 1, "green": 0, "blue": 0, "alpha": 1}));
    let (doc, _) = open(vec![plain, red]);
    assert!(matches!(node(&doc, "Plain").kind, NodeKind::Frame));
    assert!(node(&doc, "Plain").clip);
    assert_eq!(solid(&doc, "Plain"), Some([1.0, 1.0, 1.0, 1.0]));
    assert_eq!(solid(&doc, "Red"), Some([1.0, 0.0, 0.0, 1.0]));
    let b = doc.world_bounds(node(&doc, "Red").id).unwrap();
    assert_eq!((b.x0, b.y0, b.width(), b.height()), (200.0, 0.0, 100.0, 50.0));
}

#[test]
fn group_children_positions_accumulate() {
    let r = rect("R", 5.0, 5.0, 10.0, 10.0);
    let g = group("G", 10.0, 20.0, 15.0, 15.0, vec![r]);
    let (doc, _) = open(vec![artboard("A", "Board", 100.0, 100.0, 300.0, 300.0, vec![g])]);
    assert!(matches!(node(&doc, "G").kind, NodeKind::Group));
    let b = doc.world_bounds(node(&doc, "R").id).unwrap();
    assert!((b.x0 - 115.0).abs() < 1e-6 && (b.y0 - 125.0).abs() < 1e-6, "{b:?}");
}

#[test]
fn rotation_and_flip() {
    let rotated = with(rect("Turned", 0.0, 0.0, 40.0, 20.0), "rotation", json!(90));
    let flipped = with(rect("Flipped", 100.0, 0.0, 40.0, 20.0), "isFlippedHorizontal", json!(true));
    let (doc, _) = open(vec![artboard("A", "Board", 0.0, 0.0, 300.0, 300.0, vec![rotated, flipped])]);
    let b = doc.world_bounds(node(&doc, "Turned").id).unwrap();
    assert!((b.width() - 20.0).abs() < 1e-6 && (b.height() - 40.0).abs() < 1e-6, "{b:?}");
    assert!((b.center().x - 20.0).abs() < 1e-6 && (b.center().y - 10.0).abs() < 1e-6, "{b:?}");
    // Counter-clockwise on screen: the box's x axis points up.
    let t = doc.world_transform(node(&doc, "Turned").id).as_coeffs();
    assert!(t[1] < -0.99, "{t:?}");
    let t = doc.world_transform(node(&doc, "Flipped").id).as_coeffs();
    assert!(t[0] < -0.99, "{t:?}");
    let b = doc.world_bounds(node(&doc, "Flipped").id).unwrap();
    assert!((b.x0 - 100.0).abs() < 1e-6 && (b.width() - 40.0).abs() < 1e-6, "{b:?}");
}

#[test]
fn resizing_constraint_bits() {
    use HConstraint as H;
    use VConstraint as V;
    // A cleared bit pins: right 1, width 2, left 4, bottom 8, height 16, top 32.
    let cases = [
        (63, H::Scale, V::Scale),
        (63 - 4 - 32, H::Left, V::Top),
        (63 - 1 - 8, H::Right, V::Bottom),
        (63 - 1 - 4, H::LeftRight, V::Scale),
        (63 - 8 - 32, H::Scale, V::TopBottom),
        (63 - 2 - 16, H::Center, V::Center),
        (63 - 1 - 2, H::Right, V::Scale),
        (0, H::LeftRight, V::TopBottom),
    ];
    let layers: Vec<Value> = cases
        .iter()
        .enumerate()
        .map(|(i, (mask, _, _))| with(rect(&format!("R{i}"), 0.0, 0.0, 10.0, 10.0), "resizingConstraint", json!(mask)))
        .collect();
    let (doc, _) = open(vec![artboard("A", "Board", 0.0, 0.0, 100.0, 100.0, layers)]);
    for (i, (mask, h, v)) in cases.iter().enumerate() {
        let c = node(&doc, &format!("R{i}")).constraints;
        assert_eq!((c.horizontal, c.vertical), (*h, *v), "mask {mask}");
    }
}

#[test]
fn hidden_locked_named() {
    let r = with(with(rect("Hidden", 0.0, 0.0, 10.0, 10.0), "isVisible", json!(false)), "isLocked", json!(true));
    let (doc, _) = open(vec![artboard("A", "Board", 0.0, 0.0, 100.0, 100.0, vec![r])]);
    let n = node(&doc, "Hidden");
    assert!(!n.visible && n.locked);
}

#[test]
fn bad_numbers_are_sane() {
    let mut nan = rect("Junk", 0.0, 0.0, 10.0, 10.0);
    nan["frame"]["width"] = json!("NaN");
    nan["frame"]["x"] = json!(1e300);
    let mut neg = rect("Negative", 0.0, 0.0, 10.0, 10.0);
    neg["frame"]["height"] = json!(-5);
    neg["rotation"] = json!("x");
    let mut none = rect("Frameless", 0.0, 0.0, 10.0, 10.0);
    none.as_object_mut().unwrap().remove("frame");
    let (doc, _) = open(vec![artboard("A", "Board", 0.0, 0.0, 100.0, 100.0, vec![nan, neg, none])]);
    for n in doc.nodes() {
        assert!(n.width.is_finite() && n.height.is_finite() && n.width >= 0.0 && n.height >= 0.0, "{}", n.name);
        assert!(n.transform.as_coeffs().iter().all(|c| c.is_finite()), "{}", n.name);
    }
}

#[test]
fn rectangle_corner_radii() {
    let mut each = rect("Each", 0.0, 0.0, 40.0, 40.0);
    for (i, r) in [1, 2, 3, 4].iter().enumerate() {
        each["points"][i]["cornerRadius"] = json!(r);
    }
    let fixed = with(rect("Fixed", 50.0, 0.0, 40.0, 40.0), "fixedRadius", json!(8));
    let (doc, _) = open(vec![artboard("A", "Board", 0.0, 0.0, 100.0, 100.0, vec![each, fixed])]);
    assert!(matches!(node(&doc, "Each").kind, NodeKind::Rectangle));
    assert_eq!(node(&doc, "Each").corners.radii, [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(node(&doc, "Fixed").corners.radii, [8.0; 4]);
}

#[test]
fn oval_slice_and_bitmap() {
    let mut oval = rect("Round", 0.0, 0.0, 30.0, 20.0);
    oval["_class"] = json!("oval");
    let mut slice = rect("Cut", 0.0, 0.0, 30.0, 20.0);
    slice["_class"] = json!("slice");
    let mut bitmap = rect("Photo", 0.0, 0.0, 30.0, 20.0);
    bitmap["_class"] = json!("bitmap");
    bitmap["image"] = json!({"_class": "MSJSONFileReference", "_ref_class": "MSImageData", "_ref": "images/abc123.png"});
    let png = {
        let mut out = Vec::new();
        image::RgbaImage::new(2, 2).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    };
    let (doc, _) = open_with(vec![artboard("A", "Board", 0.0, 0.0, 100.0, 100.0, vec![oval, slice, bitmap])], &[("images/abc123.png", &png)]);
    assert!(matches!(node(&doc, "Round").kind, NodeKind::Ellipse { .. }));
    assert!(matches!(node(&doc, "Cut").kind, NodeKind::Slice));
    let photo = node(&doc, "Photo");
    assert!(matches!(photo.kind, NodeKind::Rectangle));
    assert!(photo.fills.iter().any(|p| matches!(&p.kind, PaintKind::Image(_))));
    assert_eq!(doc.images.len(), 1);
}

#[test]
fn unknown_class_is_a_warning() {
    let mut odd = rect("Odd", 0.0, 0.0, 10.0, 10.0);
    odd["_class"] = json!("MSSomethingNew");
    let (doc, warnings) = open(vec![artboard("A", "Board", 0.0, 0.0, 100.0, 100.0, vec![odd])]);
    assert!(doc.nodes().all(|n| n.name != "Odd"));
    assert!(warnings.iter().any(|w| w.contains("MSSomethingNew")), "{warnings:?}");
}

#[test]
fn sketch_2025_frames_and_graphics() {
    let mut frame = artboard("F", "Frame", 0.0, 0.0, 100.0, 100.0, vec![rect("Inside", 0.0, 0.0, 10.0, 10.0)]);
    frame["_class"] = json!("frame");
    frame["clipsContent"] = json!(false);
    let mut graphic = group("Graphic", 10.0, 10.0, 10.0, 10.0, vec![rect("Leaf", 0.0, 0.0, 10.0, 10.0)]);
    graphic["_class"] = json!("graphic");
    frame["layers"].as_array_mut().unwrap().push(graphic);
    let (doc, _) = open(vec![frame]);
    assert!(matches!(node(&doc, "Frame").kind, NodeKind::Frame));
    assert!(!node(&doc, "Frame").clip);
    assert!(matches!(node(&doc, "Graphic").kind, NodeKind::Group));
}
