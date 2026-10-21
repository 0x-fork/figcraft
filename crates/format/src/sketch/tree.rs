//! Sketch pages and layers → REST API file JSON nodes.

use figcraft_doc::VectorNetwork;
use figcraft_doc::geom::{Affine, Rect};
use serde_json::{Map as JsonMap, Value, json};

use super::Archive;

/// Deepest layer nesting followed (deeper layers are skipped with a warning).
const MAX_DEPTH: usize = 200;
/// Largest size and farthest position kept (the REST importer's own bounds).
const MAX_SIZE: f64 = 1e6;
const MAX_COORD: f64 = 1e7;

/// The REST JSON of a Sketch file, with what the mapping noticed.
pub(crate) struct Map {
    pub json: Value,
    pub warnings: Vec<String>,
    /// Vector networks the JSON references by `vectorNetworkRef`.
    pub networks: Vec<Option<VectorNetwork>>,
    /// A text layer stored the pre-48 way was met: the file is refused.
    pub too_old: bool,
}

/// The mapping's state while walking the file.
pub(crate) struct Cx {
    pub warnings: Vec<String>,
    pub networks: Vec<Option<VectorNetwork>>,
    pub too_old: bool,
    /// Layers mapped so far, and the most the file may give.
    pub nodes: usize,
    pub max_nodes: usize,
}

impl Cx {
    pub(crate) fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }
}

/// Map the pages of `archive` to a REST file JSON named `name`.
pub(crate) fn to_rest(archive: &Archive, name: &str, max_nodes: usize) -> Map {
    let mut cx = Cx { warnings: Vec::new(), networks: Vec::new(), too_old: false, nodes: 0, max_nodes };
    let mut canvases = Vec::new();
    for (id, page) in &archive.pages {
        let mut node = JsonMap::new();
        node.insert("type".into(), json!("CANVAS"));
        node.insert("id".into(), json!(id));
        node.insert("name".into(), json!(str_field(page, "name").unwrap_or("Page")));
        let kids: Vec<Value> = layers(page).iter().filter_map(|l| layer(&mut cx, l, Affine::IDENTITY, 0, true)).collect();
        node.insert("children".into(), Value::Array(kids));
        canvases.push(Value::Object(node));
    }
    let json = json!({"name": name, "document": {"type": "DOCUMENT", "id": "0:0", "children": canvases}});
    Map { json, warnings: cx.warnings, networks: cx.networks, too_old: cx.too_old }
}

/// One Sketch layer as a REST node (with its children), or `None` when it is skipped. `parent`:
/// the parent layer's transform to the page; `root`: the layer sits on the page itself.
pub(crate) fn layer(cx: &mut Cx, l: &Value, parent: Affine, depth: usize, root: bool) -> Option<Value> {
    if depth > MAX_DEPTH {
        cx.warn(format!("layers nested deeper than {MAX_DEPTH} levels were skipped"));
        return None;
    }
    if is_archived_text(l) {
        cx.too_old = true;
        return None;
    }
    let class = str_field(l, "_class")?;
    let ty = match class {
        "artboard" | "frame" => "FRAME",
        "group" | "graphic" => "GROUP",
        "rectangle" | "bitmap" => "RECTANGLE",
        "oval" => "ELLIPSE",
        "slice" => "SLICE",
        other => {
            cx.warn(format!("Sketch {} layers are not supported yet and were skipped", clip(other)));
            return None;
        }
    };
    if cx.nodes >= cx.max_nodes {
        cx.warn(format!("the file has more than {} layers; the rest were skipped", cx.max_nodes));
        return None;
    }
    cx.nodes += 1;
    let (w, h) = size(l);
    let world = parent * local(l, w, h);
    let mut node = JsonMap::new();
    node.insert("type".into(), json!(ty));
    if let Some(id) = str_field(l, "do_objectID") {
        node.insert("id".into(), json!(id));
    }
    node.insert("name".into(), json!(str_field(l, "name").unwrap_or(class)));
    node.insert("visible".into(), json!(bool_field(l, "isVisible").unwrap_or(true)));
    node.insert("locked".into(), json!(bool_field(l, "isLocked").unwrap_or(false)));
    node.insert("size".into(), json!({"x": w, "y": h}));
    let rel = parent.inverse() * world;
    let [a, b, c, d, e, f] = rel.as_coeffs();
    node.insert("relativeTransform".into(), json!([[a, c, e], [b, d, f]]));
    // The page places top-level layers by their transform; deeper ones also say where they end up,
    // which tells the importer their transform is relative to their group.
    if !root {
        let bb = world.transform_rect_bbox(Rect::new(0.0, 0.0, w, h));
        node.insert("absoluteBoundingBox".into(), json!({"x": bb.x0, "y": bb.y0, "width": bb.width(), "height": bb.height()}));
    }
    node.insert("constraints".into(), constraints(l.get("resizingConstraint").and_then(Value::as_i64).unwrap_or(63)));
    match class {
        "artboard" | "frame" => {
            let clips = if class == "artboard" { true } else { bool_field(l, "clipsContent").unwrap_or(true) };
            node.insert("clipsContent".into(), json!(clips));
            let background = (bool_field(l, "hasBackgroundColor") == Some(true)).then(|| l.get("backgroundColor").and_then(color)).flatten();
            if class == "artboard" || background.is_some() {
                let c = background.unwrap_or_else(|| json!({"r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0}));
                node.insert("fills".into(), json!([{"type": "SOLID", "color": c}]));
            }
        }
        "rectangle" => corner_radii(l, &mut node),
        "bitmap" => match l.get("image").and_then(|i| str_field(i, "_ref")) {
            Some(r) => {
                let paint = json!({"type": "IMAGE", "scaleMode": "STRETCH", "imageRef": r, "imageTransform": [[1, 0, 0], [0, 1, 0]]});
                node.insert("fills".into(), json!([paint]));
            }
            None => cx.warn("images stored inside the JSON were skipped"),
        },
        _ => {}
    }
    if matches!(ty, "FRAME" | "GROUP") {
        let kids: Vec<Value> = layers(l).iter().filter_map(|c| layer(cx, c, world, depth + 1, false)).collect();
        node.insert("children".into(), Value::Array(kids));
    }
    Some(Value::Object(node))
}

/// A layer's size from its `frame` (finite, not negative, at most [`MAX_SIZE`]).
fn size(l: &Value) -> (f64, f64) {
    let f = l.get("frame");
    let n = |k: &str| f.and_then(|f| f.get(k)).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, MAX_SIZE);
    (n("width"), n("height"))
}

/// A layer's transform within its parent: its frame's position, then its rotation (degrees,
/// counter-clockwise on screen) and flips about its centre.
fn local(l: &Value, w: f64, h: f64) -> Affine {
    let f = l.get("frame");
    let n = |k: &str| f.and_then(|f| f.get(k)).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(-MAX_COORD, MAX_COORD);
    let rot = l.get("rotation").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0) % 360.0;
    let sx = if bool_field(l, "isFlippedHorizontal") == Some(true) { -1.0 } else { 1.0 };
    let sy = if bool_field(l, "isFlippedVertical") == Some(true) { -1.0 } else { 1.0 };
    Affine::translate((n("x") + w / 2.0, n("y") + h / 2.0))
        * Affine::rotate(-rot.to_radians())
        * Affine::scale_non_uniform(sx, sy)
        * Affine::translate((-w / 2.0, -h / 2.0))
}

/// REST constraints from Sketch's `resizingConstraint`: six bits, a cleared bit pins that side or
/// fixes that size (right 1, width 2, left 4, bottom 8, height 16, top 32). A layer pinned on
/// neither side keeps its place in proportion (scale) unless its size is fixed (centre).
fn constraints(mask: i64) -> Value {
    let pinned = |bit: i64| mask & bit == 0;
    let axis = |low: i64, size: i64, high: i64, low_name: &str, high_name: &str, both: &str| match (pinned(low), pinned(high)) {
        (true, true) => both.to_string(),
        (true, false) => low_name.to_string(),
        (false, true) => high_name.to_string(),
        (false, false) if pinned(size) => "CENTER".into(),
        (false, false) => "SCALE".into(),
    };
    json!({
        "horizontal": axis(4, 2, 1, "LEFT", "RIGHT", "LEFT_RIGHT"),
        "vertical": axis(32, 16, 8, "TOP", "BOTTOM", "TOP_BOTTOM"),
    })
}

/// A rectangle's corner radii: per corner from its points (top left, top right, bottom right,
/// bottom left), else its `fixedRadius`.
fn corner_radii(l: &Value, node: &mut JsonMap<String, Value>) {
    let radius = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|r| r.is_finite()).map_or(0.0, |r| r.clamp(0.0, MAX_SIZE));
    let pts = l.get("points").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let each: Vec<f64> = pts.iter().map(|p| radius(p.get("cornerRadius"))).collect();
    if each.len() == 4 && each.iter().any(|r| *r > 0.0) {
        node.insert("rectangleCornerRadii".into(), json!(each));
    } else if radius(l.get("fixedRadius")) > 0.0 {
        node.insert("cornerRadius".into(), json!(radius(l.get("fixedRadius"))));
    }
}

/// A Sketch colour (`red`, `green`, `blue`, `alpha` in 0–1) as a REST colour.
pub(crate) fn color(c: &Value) -> Option<Value> {
    let n = |k: &str| c.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1.0));
    Some(json!({"r": n("red")?, "g": n("green")?, "b": n("blue")?, "a": n("alpha").unwrap_or(1.0)}))
}

pub(crate) fn bool_field(v: &Value, key: &str) -> Option<bool> {
    v.get(key).and_then(Value::as_bool)
}

/// At most 64 characters of a name read from the file, for warnings.
fn clip(s: &str) -> String {
    s.chars().take(64).collect()
}

/// A text layer whose string is a binary property list (Sketch 43–47).
fn is_archived_text(l: &Value) -> bool {
    l.get("attributedString").is_some_and(|a| a.get("archivedAttributedString").is_some())
}

/// The child layers of a page or layer.
pub(crate) fn layers(l: &Value) -> &[Value] {
    l.get("layers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

pub(crate) fn str_field<'v>(v: &'v Value, key: &str) -> Option<&'v str> {
    v.get(key).and_then(Value::as_str)
}
