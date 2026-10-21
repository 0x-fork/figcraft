//! Sketch curve points → vector networks; shape groups → shapes and boolean operations.
//!
//! A path's `points` are `curvePoint`s whose `point`, `curveFrom` (the handle leaving the point)
//! and `curveTo` (the handle arriving at it) are `"{x, y}"` strings in 0–1 of the layer's frame;
//! values outside that range are allowed. Sketch 51 and older wrap every shape in a `shapeGroup`
//! that carries the style; its children combine left to right, each with its own
//! `booleanOperation` (−1 none and 0 union, 1 subtract, 2 intersect, 3 difference).

use figcraft_doc::geom::{Affine, polygon_points, star_points};
use figcraft_doc::{Region, Segment, VectorNetwork, Vertex, WindingRule};
use serde_json::{Map as JsonMap, Value, json};

use super::tree::{Cx, base, layer, layers, local, size};

/// Most points read from one path.
const MAX_POINTS: usize = 100_000;

/// What a path layer becomes.
pub(crate) enum Shape {
    Vector(VectorNetwork),
    Star { count: u32, ratio: f64 },
    Polygon { count: u32 },
}

/// `"{x, y}"` → `(x, y)`, both finite.
pub(crate) fn parse_point(s: &str) -> Option<(f64, f64)> {
    let inner = s.trim().strip_prefix('{')?.strip_suffix('}')?;
    let (x, y) = inner.split_once(',')?;
    let n = |t: &str| t.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    Some((n(x)?, n(y)?))
}

/// The network of a path layer of `w`×`h` (`None` when it has no usable points).
pub(crate) fn network(cx: &mut Cx, l: &Value, w: f64, h: f64) -> Option<VectorNetwork> {
    let pts = l.get("points").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    if pts.len() > MAX_POINTS {
        cx.warn(format!("paths with more than {MAX_POINTS} points were cut"));
    }
    let at = |p: &Value, key: &str| p.get(key).and_then(Value::as_str).and_then(parse_point).map(|(x, y)| (x * w, y * h));
    let mut vertices = Vec::new();
    // Per vertex: its handles (relative to it) when it has them.
    let mut handles: Vec<((f64, f64), (f64, f64))> = Vec::new();
    for p in pts.iter().take(MAX_POINTS) {
        let Some((x, y)) = at(p, "point") else {
            cx.warn("path points that couldn't be read were skipped");
            continue;
        };
        let curved = p.get("curveMode").and_then(Value::as_i64).is_some_and(|m| m != 1);
        let has = |flag: &str| p.get(flag).and_then(Value::as_bool).unwrap_or(curved);
        let handle = |flag: &str, key: &str| if has(flag) { at(p, key).map_or((0.0, 0.0), |(hx, hy)| (hx - x, hy - y)) } else { (0.0, 0.0) };
        handles.push((handle("hasCurveFrom", "curveFrom"), handle("hasCurveTo", "curveTo")));
        let mut v = Vertex::new(x, y);
        v.radius = p.get("cornerRadius").and_then(Value::as_f64).filter(|r| r.is_finite()).map_or(0.0, |r| r.clamp(0.0, 1e6));
        vertices.push(v);
    }
    let n = vertices.len();
    if n == 0 {
        return None;
    }
    let closed = l.get("isClosed").and_then(Value::as_bool).unwrap_or(false) && n > 1;
    let count = if closed { n } else { n - 1 };
    let segments: Vec<Segment> = (0..count)
        .map(|i| {
            let j = (i + 1) % n;
            let ta = handles.get(i).map_or((0.0, 0.0), |h| h.0);
            let tb = handles.get(j).map_or((0.0, 0.0), |h| h.1);
            Segment { a: i, b: j, ta, tb }
        })
        .collect();
    let winding = match l.get("style").and_then(|s| s.get("windingRule")).and_then(Value::as_i64) {
        Some(1) => WindingRule::EvenOdd,
        _ => WindingRule::NonZero,
    };
    let regions = if closed { vec![Region { loops: vec![(0..segments.len()).collect()], winding }] } else { Vec::new() };
    Some(VectorNetwork { vertices, segments, regions })
}

/// A path, star, polygon or triangle layer: a star or polygon while its points are still the
/// generated ones (same as FigCraft's, within half a percent), else an editable vector.
pub(crate) fn shape(cx: &mut Cx, l: &Value, w: f64, h: f64) -> Option<Shape> {
    let net = network(cx, l, w, h)?;
    let class = l.get("_class").and_then(Value::as_str).unwrap_or("");
    let count = l.get("numberOfPoints").and_then(Value::as_u64).filter(|c| (3..=120).contains(c)).and_then(|c| u32::try_from(c).ok());
    let plain = net.vertices.iter().all(|v| v.radius == 0.0) && net.segments.iter().all(|s| s.ta == (0.0, 0.0) && s.tb == (0.0, 0.0));
    let matches = |pts: Vec<figcraft_doc::geom::Point>| {
        let tol = 0.005 * w.max(h) + 1e-6;
        pts.len() == net.vertices.len() && pts.iter().zip(&net.vertices).all(|(p, v)| (p.x - v.x).abs() <= tol && (p.y - v.y).abs() <= tol)
    };
    match (class, count) {
        ("star", Some(count)) if plain => {
            let ratio = l.get("radius").and_then(Value::as_f64).filter(|r| r.is_finite() && *r > 0.0).map_or(0.5, |r| r.min(1.0));
            if matches(star_points(w, h, count, ratio)) {
                return Some(Shape::Star { count, ratio });
            }
        }
        ("polygon", Some(count)) if plain && matches(polygon_points(w, h, count)) => return Some(Shape::Polygon { count }),
        _ => {}
    }
    Some(Shape::Vector(net))
}

/// Write `shape` into `node` (its type and geometry).
pub(crate) fn put_shape(cx: &mut Cx, shape: Shape, node: &mut JsonMap<String, Value>) {
    match shape {
        Shape::Vector(net) => {
            node.insert("type".into(), json!("VECTOR"));
            node.insert("vectorNetworkRef".into(), json!(cx.networks.len()));
            cx.networks.push(Some(net));
        }
        Shape::Star { count, ratio } => {
            node.insert("type".into(), json!("STAR"));
            node.insert("pointCount".into(), json!(count));
            node.insert("innerRadius".into(), json!(ratio));
        }
        Shape::Polygon { count } => {
            node.insert("type".into(), json!("REGULAR_POLYGON"));
            node.insert("pointCount".into(), json!(count));
        }
    }
}

/// A `shapeGroup`: its only child carrying the group's place, name and style, or its children
/// combined by boolean operations (mixed operations nest from the left).
pub(crate) fn shape_group(cx: &mut Cx, l: &Value, parent: Affine, depth: usize, root: bool) -> Option<Value> {
    let (w, h) = size(l);
    let world = parent * local(l, w, h);
    let kids = layers(l);
    if let [only] = kids {
        let mut node = layer(cx, only, world, depth + 1, false)?;
        let (cw, ch) = size(only);
        let child_world = world * local(only, cw, ch);
        let group = base(l, "GROUP", parent, child_world, cw, ch, root);
        if let Some(m) = node.as_object_mut() {
            for key in ["id", "name", "relativeTransform", "absoluteBoundingBox", "constraints", "locked"] {
                match group.get(key) {
                    Some(v) => m.insert(key.into(), v.clone()),
                    None => m.remove(key),
                };
            }
            let visible = group.get("visible").and_then(Value::as_bool).unwrap_or(true) && m.get("visible").and_then(Value::as_bool).unwrap_or(true);
            m.insert("visible".into(), json!(visible));
            if l.get("style").is_some() {
                super::style::clear(m);
                m.remove("styles");
                super::tree::styles(cx, l, cw, ch, m);
            }
            super::tree::mask(l, m);
        }
        return Some(node);
    }
    let mut chain: Vec<Value> = Vec::new();
    let mut op = "UNION";
    for (i, c) in kids.iter().enumerate() {
        let Some(child) = layer(cx, c, world, depth + 2, false) else { continue };
        let this = boolean_op(c);
        if i > 0 && this != op && chain.len() > 1 {
            let inner = synthetic(l, op, parent, world, w, h, std::mem::take(&mut chain));
            chain.push(inner);
        }
        if i > 0 {
            op = this;
        }
        chain.push(child);
    }
    if chain.is_empty() {
        return None;
    }
    let mut node = base(l, "BOOLEAN_OPERATION", parent, world, w, h, root);
    node.insert("booleanOperation".into(), json!(op));
    node.insert("children".into(), Value::Array(chain));
    super::tree::mask(l, &mut node);
    super::tree::styles(cx, l, w, h, &mut node);
    Some(Value::Object(node))
}

/// A boolean operation the shape group doesn't have as a layer of its own: placed like the group.
fn synthetic(l: &Value, op: &str, parent: Affine, world: Affine, w: f64, h: f64, children: Vec<Value>) -> Value {
    let mut node = base(l, "BOOLEAN_OPERATION", parent, world, w, h, false);
    node.remove("id");
    let name = match op {
        "SUBTRACT" => "Subtract",
        "INTERSECT" => "Intersect",
        "EXCLUDE" => "Exclude",
        _ => "Union",
    };
    node.insert("name".into(), json!(name));
    // Placed where the group is: inside it, that's no move at all.
    node.insert("relativeTransform".into(), json!([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]));
    node.insert("booleanOperation".into(), json!(op));
    node.insert("children".into(), Value::Array(children));
    Value::Object(node)
}

fn boolean_op(l: &Value) -> &'static str {
    match l.get("booleanOperation").and_then(Value::as_i64) {
        Some(1) => "SUBTRACT",
        Some(2) => "INTERSECT",
        Some(3) => "EXCLUDE",
        _ => "UNION",
    }
}
