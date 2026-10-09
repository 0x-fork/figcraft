//! Vector edit mode: editing a vector network's vertices and bezier handles on the canvas.
//!
//! Entering edit mode on a non-vector shape converts it to a vector first (like the incumbent).
//! Vertex coordinates are node-local; after each edit the node is re-fitted so its box wraps the
//! network again.

use figcraft_doc::*;
use figcraft_geom::{Affine, Point, Rect, Vec2};
use serde_json::{Value, json};

use crate::cmd::CommandSpec;
use crate::params::*;
use crate::{EngineError, Result, Session, bad, spec};

/// What a pointer is over in edit mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VHit {
    Vertex(usize),
    /// Handle of segment `seg`; `start` = the handle at the segment's `a` end.
    Handle {
        seg: usize,
        start: bool,
    },
    Segment {
        seg: usize,
        t: f64,
    },
}

/// Convert a shape to an editable vector (keeps paints, transform and look).
pub fn ensure_vector(doc: &mut Document, id: NodeId) -> Result<()> {
    let n = doc.get(id)?.clone();
    if matches!(n.kind, NodeKind::Vector { .. }) {
        return Ok(());
    }
    if !n.kind.is_shape() && !matches!(n.kind, NodeKind::BooleanOp { .. }) {
        return Err(EngineError::Other(format!("{} layers can't be edited as vectors", n.kind.label())));
    }
    let (path, rule) = figcraft_render::node_outline(doc, &n).ok_or_else(|| EngineError::Other("no outline".into()))?;
    let mut net = VectorNetwork::from_path(&path);
    if matches!(n.kind, NodeKind::Line) {
        net.regions.clear();
    }
    if rule == figcraft_render::FillRule::EvenOdd {
        for r in &mut net.regions {
            r.winding = WindingRule::EvenOdd;
        }
    }
    let node = doc.get_mut(id)?;
    node.kind = NodeKind::Vector { network: net };
    node.children.clear();
    if matches!(n.kind, NodeKind::Line) {
        node.stroke.align = StrokeAlign::Center;
    }
    node.corners = Corners::default();
    refit(doc, id)?;
    Ok(())
}

/// Make the node's box wrap its network again (moving the node so nothing shifts on screen).
pub fn refit(doc: &mut Document, id: NodeId) -> Result<()> {
    let n = doc.get_mut(id)?;
    let NodeKind::Vector { network } = &mut n.kind else { return Ok(()) };
    let Some(b) = network.bounds() else { return Ok(()) };
    if !(b.x0.is_finite() && b.y0.is_finite()) {
        return Ok(());
    }
    if b.x0.abs() > 1e-9 || b.y0.abs() > 1e-9 {
        network.translate(-b.x0, -b.y0);
        n.transform *= Affine::translate((b.x0, b.y0));
    }
    n.width = b.width();
    n.height = b.height();
    Ok(())
}

fn network(doc: &Document, id: NodeId) -> Option<&VectorNetwork> {
    match &doc.node(id)?.kind {
        NodeKind::Vector { network } => Some(network),
        _ => None,
    }
}

fn network_mut(doc: &mut Document, id: NodeId) -> Option<&mut VectorNetwork> {
    match &mut doc.node_mut(id)?.kind {
        NodeKind::Vector { network } => Some(network),
        _ => None,
    }
}

/// Hit test in page coordinates with a tolerance in page units. Handles of selected vertices win,
/// then vertices, then segments.
pub fn hit(doc: &Document, id: NodeId, selected: &[usize], p: Point, tol: f64) -> Option<VHit> {
    let net = network(doc, id)?;
    let w = doc.world_transform(id);
    let lp = w.inverse() * p;
    // Tolerance in local units (approximate for non-uniform scale).
    let s = (w.determinant().abs()).sqrt().max(1e-9);
    let tl = tol / s;
    for (si, seg) in net.segments.iter().enumerate() {
        for (start, vi, t) in [(true, seg.a, seg.ta), (false, seg.b, seg.tb)] {
            if !selected.contains(&vi) || t == (0.0, 0.0) {
                continue;
            }
            let Some(v) = net.vertices.get(vi) else { continue };
            if (Point::new(v.x + t.0, v.y + t.1) - lp).hypot() <= tl {
                return Some(VHit::Handle { seg: si, start });
            }
        }
    }
    for (vi, v) in net.vertices.iter().enumerate() {
        if (v.point() - lp).hypot() <= tl {
            return Some(VHit::Vertex(vi));
        }
    }
    use figcraft_geom::kurbo::{CubicBez, ParamCurveNearest};
    for (si, seg) in net.segments.iter().enumerate() {
        let (Some(a), Some(b)) = (net.vertices.get(seg.a), net.vertices.get(seg.b)) else { continue };
        let c = CubicBez::new(a.point(), Point::new(a.x + seg.ta.0, a.y + seg.ta.1), Point::new(b.x + seg.tb.0, b.y + seg.tb.1), b.point());
        let near = c.nearest(lp, 0.01);
        if near.distance_sq.sqrt() <= tl {
            return Some(VHit::Segment { seg: si, t: near.t });
        }
    }
    None
}

/// Move vertices by `d` (local units); their handles move with them.
pub fn move_vertices(doc: &mut Document, id: NodeId, idx: &[usize], d: Vec2) -> Result<()> {
    let net = network_mut(doc, id).ok_or_else(|| EngineError::Other("not a vector".into()))?;
    for i in idx {
        if let Some(v) = net.vertices.get_mut(*i) {
            v.x += d.x;
            v.y += d.y;
        }
    }
    Ok(())
}

/// Set a handle to absolute local position `p`; the opposite handle at the same vertex mirrors
/// (angle and length) unless `independent`.
pub fn set_handle(doc: &mut Document, id: NodeId, seg: usize, start: bool, p: Point, independent: bool) -> Result<()> {
    let net = network_mut(doc, id).ok_or_else(|| EngineError::Other("not a vector".into()))?;
    let Some(s) = net.segments.get(seg).copied() else { return Err(EngineError::Other("no such segment".into())) };
    let vi = if start { s.a } else { s.b };
    let Some(v) = net.vertices.get(vi).copied() else { return Ok(()) };
    let rel = (p.x - v.x, p.y - v.y);
    if let Some(sm) = net.segments.get_mut(seg) {
        if start {
            sm.ta = rel;
        } else {
            sm.tb = rel;
        }
    }
    if !independent {
        // Mirror on the other segment meeting at this vertex (smooth node).
        let others: Vec<(usize, bool)> = net
            .segments
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != seg)
            .filter_map(|(i, o)| {
                if o.a == vi {
                    Some((i, true))
                } else if o.b == vi {
                    Some((i, false))
                } else {
                    None
                }
            })
            .collect();
        if let [(oi, ostart)] = others.as_slice()
            && let Some(o) = net.segments.get_mut(*oi)
        {
            let m = (-rel.0, -rel.1);
            if *ostart {
                o.ta = m;
            } else {
                o.tb = m;
            }
        }
    }
    Ok(())
}

/// Bend a segment so its curve passes through `p` at parameter `t` (the Bend tool).
pub fn bend(doc: &mut Document, id: NodeId, seg: usize, t: f64, p: Point) -> Result<()> {
    let net = network_mut(doc, id).ok_or_else(|| EngineError::Other("not a vector".into()))?;
    let Some(s) = net.segments.get(seg).copied() else { return Ok(()) };
    let (Some(a), Some(b)) = (net.vertices.get(s.a).copied(), net.vertices.get(s.b).copied()) else { return Ok(()) };
    let t = t.clamp(0.05, 0.95);
    // Controls at the chord's thirds plus a shared offset: B(t) = L(t) + 3t(1-t)·off.
    let (a, b) = (a.point(), b.point());
    let k = 3.0 * t * (1.0 - t);
    let line = a.lerp(b, t);
    let off = (p - line) / k;
    let c1 = a + (b - a) / 3.0 + off;
    let c2 = b + (a - b) / 3.0 + off;
    if let Some(sm) = net.segments.get_mut(seg) {
        sm.ta = (c1.x - a.x, c1.y - a.y);
        sm.tb = (c2.x - b.x, c2.y - b.y);
    }
    Ok(())
}

/// Delete vertices, joining their neighbours where a vertex had exactly two segments ("heal").
pub fn delete_vertices(doc: &mut Document, id: NodeId, idx: &[usize], heal: bool) -> Result<()> {
    let net = network_mut(doc, id).ok_or_else(|| EngineError::Other("not a vector".into()))?;
    let mut idx: Vec<usize> = idx.to_vec();
    idx.sort_unstable();
    idx.dedup();
    for &vi in idx.iter().rev() {
        if vi >= net.vertices.len() {
            continue;
        }
        let touching: Vec<usize> = net.segments.iter().enumerate().filter(|(_, s)| s.a == vi || s.b == vi).map(|(i, _)| i).collect();
        let mut new_seg = None;
        if heal && let [s1, s2] = touching.as_slice() {
            let (Some(x), Some(y)) = (net.segments.get(*s1).copied(), net.segments.get(*s2).copied()) else { continue };
            let o1 = if x.a == vi { (x.b, x.tb) } else { (x.a, x.ta) };
            let o2 = if y.a == vi { (y.b, y.tb) } else { (y.a, y.ta) };
            new_seg = Some(Segment { a: o1.0, b: o2.0, ta: o1.1, tb: o2.1 });
        }
        // Remove touching segments (descending) and fix region loops.
        let mut seg_map: Vec<Option<usize>> = (0..net.segments.len()).map(Some).collect();
        for &si in touching.iter().rev() {
            net.segments.remove(si);
            if let Some(m) = seg_map.get_mut(si) {
                *m = None;
            }
        }
        for (k, m) in seg_map.iter_mut().flatten().enumerate() {
            *m = k;
        }
        let healed_index = new_seg.map(|s| {
            net.segments.push(s);
            net.segments.len() - 1
        });
        for r in &mut net.regions {
            for lp in &mut r.loops {
                let mut out = Vec::new();
                let mut inserted = false;
                for si in lp.iter() {
                    match seg_map.get(*si).copied().flatten() {
                        Some(n) => out.push(n),
                        None => {
                            if let (Some(h), false) = (healed_index, inserted) {
                                out.push(h);
                                inserted = true;
                            }
                        }
                    }
                }
                *lp = out;
            }
            r.loops.retain(|l| l.len() >= 2);
        }
        net.regions.retain(|r| !r.loops.is_empty());
        // Remove the vertex and shift indices.
        net.vertices.remove(vi);
        for s in &mut net.segments {
            if s.a > vi {
                s.a -= 1;
            }
            if s.b > vi {
                s.b -= 1;
            }
        }
    }
    net.sanitize();
    Ok(())
}

/// Split segment `seg` at `t`, inserting a vertex; returns its index.
pub fn split_segment(doc: &mut Document, id: NodeId, seg: usize, t: f64) -> Result<usize> {
    use figcraft_geom::kurbo::{CubicBez, ParamCurve};
    let net = network_mut(doc, id).ok_or_else(|| EngineError::Other("not a vector".into()))?;
    let s = net.segments.get(seg).copied().ok_or_else(|| EngineError::Other("no such segment".into()))?;
    let (Some(a), Some(b)) = (net.vertices.get(s.a).copied(), net.vertices.get(s.b).copied()) else {
        return Err(EngineError::Other("bad segment".into()));
    };
    let c = CubicBez::new(a.point(), Point::new(a.x + s.ta.0, a.y + s.ta.1), Point::new(b.x + s.tb.0, b.y + s.tb.1), b.point());
    let t = t.clamp(0.01, 0.99);
    let l = c.subsegment(0.0..t);
    let r = c.subsegment(t..1.0);
    net.vertices.push(Vertex::new(l.p3.x, l.p3.y));
    let mid = net.vertices.len() - 1;
    let straight = s.is_straight();
    if let Some(sm) = net.segments.get_mut(seg) {
        sm.b = mid;
        if straight {
            sm.ta = (0.0, 0.0);
            sm.tb = (0.0, 0.0);
        } else {
            sm.ta = (l.p1.x - l.p0.x, l.p1.y - l.p0.y);
            sm.tb = (l.p2.x - l.p3.x, l.p2.y - l.p3.y);
        }
    }
    let new_seg = if straight {
        Segment::line(mid, s.b)
    } else {
        Segment { a: mid, b: s.b, ta: (r.p1.x - r.p0.x, r.p1.y - r.p0.y), tb: (r.p2.x - r.p3.x, r.p2.y - r.p3.y) }
    };
    net.segments.push(new_seg);
    let ni = net.segments.len() - 1;
    for reg in &mut net.regions {
        for lp in &mut reg.loops {
            if let Some(pos) = lp.iter().position(|x| *x == seg) {
                lp.insert(pos + 1, ni);
            }
        }
    }
    Ok(mid)
}

/// Vertices inside a page-space rectangle.
pub fn vertices_in(doc: &Document, id: NodeId, r: Rect) -> Vec<usize> {
    let Some(net) = network(doc, id) else { return vec![] };
    let w = doc.world_transform(id);
    net.vertices.iter().enumerate().filter(|(_, v)| r.contains(w * v.point())).map(|(i, _)| i).collect()
}

/// Page-space geometry of the network for drawing the edit overlay: segments (as cubic control
/// points), vertices, and handles of the selected vertices.
pub fn overlay(doc: &Document, id: NodeId, selected: &[usize]) -> Option<Value> {
    let net = network(doc, id)?;
    let w = doc.world_transform(id);
    let p = |x: f64, y: f64| {
        let q = w * Point::new(x, y);
        json!([q.x, q.y])
    };
    let segs: Vec<Value> = net
        .segments
        .iter()
        .filter_map(|s| {
            let (a, b) = (net.vertices.get(s.a)?, net.vertices.get(s.b)?);
            Some(json!([p(a.x, a.y), p(a.x + s.ta.0, a.y + s.ta.1), p(b.x + s.tb.0, b.y + s.tb.1), p(b.x, b.y)]))
        })
        .collect();
    let verts: Vec<Value> = net.vertices.iter().map(|v| p(v.x, v.y)).collect();
    let mut handles = Vec::new();
    for s in &net.segments {
        for (vi, t) in [(s.a, s.ta), (s.b, s.tb)] {
            if selected.contains(&vi)
                && t != (0.0, 0.0)
                && let Some(v) = net.vertices.get(vi)
            {
                handles.push(json!([p(v.x, v.y), p(v.x + t.0, v.y + t.1)]));
            }
        }
    }
    Some(json!({"segments": segs, "vertices": verts, "handles": handles, "selected": selected}))
}

/// Start editing `id` as a vector.
pub fn begin(s: &mut Session, id: NodeId) -> Result<()> {
    let before = s.ds()?.doc.clone();
    {
        let doc = s.doc_mut()?;
        ensure_vector(doc, id)?;
    }
    let ds = s.ds_mut()?;
    if !std::sync::Arc::ptr_eq(&before, &ds.doc) {
        ds.history.undo.push(crate::HistoryEntry { label: "Edit vector".into(), doc: before, selection: ds.selection.clone(), page: ds.page });
        ds.history.redo.clear();
    }
    ds.vector_edit = Some((id, vec![]));
    ds.selection = vec![id];
    ds.text_edit = None;
    ds.revision += 1;
    Ok(())
}

pub fn end(s: &mut Session) -> Result<()> {
    let ds = s.ds_mut()?;
    ds.vector_edit = None;
    ds.revision += 1;
    Ok(())
}

fn editing(s: &Session) -> Result<(NodeId, Vec<usize>)> {
    s.ds()?.vector_edit.clone().ok_or_else(|| EngineError::Disabled("vector".into(), "not in vector edit mode".into()))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!("vector.edit.begin", "Edit vector", "{id?} (converts shapes to vectors)", false, |s, p| {
            let id = targets(s, p).first().copied().ok_or_else(|| bad("vector.edit.begin", "select a shape"))?;
            begin(s, id)?;
            Ok(json!({"id": id}))
        }),
        spec!("vector.edit.end", "Done editing", "{}", false, |s, _| {
            end(s)?;
            Ok(json!({"ok": true}))
        }),
        spec!("vector.edit.state", "Vector edit state", "{}", false, |s, _| {
            let (id, sel) = editing(s)?;
            Ok(overlay(s.doc()?, id, &sel).unwrap_or(Value::Null))
        }),
        spec!("vector.edit.select", "Select vertices", "{vertices: [i, ...]}", false, |s, p| {
            let (id, _) = editing(s)?;
            let v: Vec<usize> = match get(p, "vertices") {
                Some(Value::Array(a)) => a.iter().filter_map(f64_of).map(|x| x.max(0.0) as usize).collect(),
                _ => vec![],
            };
            let ds = s.ds_mut()?;
            ds.vector_edit = Some((id, v));
            ds.revision += 1;
            Ok(json!({"ok": true}))
        }),
        spec!("vector.edit.moveVertices", "Move vertices", "{vertices?, dx, dy} (local units; default the selected vertices)", true, |s, p| {
            let (id, sel) = editing(s)?;
            let v: Vec<usize> = match get(p, "vertices") {
                Some(Value::Array(a)) => a.iter().filter_map(f64_of).map(|x| x.max(0.0) as usize).collect(),
                _ => sel,
            };
            let d = Vec2::new(num_or(p, "dx", 0.0), num_or(p, "dy", 0.0));
            let doc = s.doc_mut()?;
            move_vertices(doc, id, &v, d)?;
            refit(doc, id)?;
            Ok(json!({"ok": true}))
        }),
        spec!("vector.edit.setHandle", "Set handle", "{segment, start (bool), x, y (local), independent?}", true, |s, p| {
            let (id, _) = editing(s)?;
            let seg = req_num("vector.edit.setHandle", p, "segment")?.max(0.0) as usize;
            let start = boolean(p, "start").unwrap_or(true);
            let pt = Point::new(req_num("vector.edit.setHandle", p, "x")?, req_num("vector.edit.setHandle", p, "y")?);
            let doc = s.doc_mut()?;
            set_handle(doc, id, seg, start, pt, boolean(p, "independent").unwrap_or(false))?;
            refit(doc, id)?;
            Ok(json!({"ok": true}))
        }),
        spec!("vector.edit.addVertex", "Add point", "{segment, t (0..1)}", true, |s, p| {
            let (id, _) = editing(s)?;
            let seg = req_num("vector.edit.addVertex", p, "segment")?.max(0.0) as usize;
            let t = num_or(p, "t", 0.5);
            let v = split_segment(s.doc_mut()?, id, seg, t)?;
            let ds = s.ds_mut()?;
            ds.vector_edit = Some((id, vec![v]));
            Ok(json!({"vertex": v}))
        }),
        spec!("vector.deleteHeal", "Delete and heal selection", "{vertices?}", true, |s, p| {
            let (id, sel) = editing(s)?;
            let v: Vec<usize> = match get(p, "vertices") {
                Some(Value::Array(a)) => a.iter().filter_map(f64_of).map(|x| x.max(0.0) as usize).collect(),
                _ => sel,
            };
            let doc = s.doc_mut()?;
            delete_vertices(doc, id, &v, true)?;
            refit(doc, id)?;
            let ds = s.ds_mut()?;
            ds.vector_edit = Some((id, vec![]));
            Ok(json!({"ok": true}))
        }),
        spec!("vector.edit.delete", "Delete points", "{vertices?}", true, |s, p| {
            let (id, sel) = editing(s)?;
            let v: Vec<usize> = match get(p, "vertices") {
                Some(Value::Array(a)) => a.iter().filter_map(f64_of).map(|x| x.max(0.0) as usize).collect(),
                _ => sel,
            };
            let doc = s.doc_mut()?;
            delete_vertices(doc, id, &v, false)?;
            refit(doc, id)?;
            let ds = s.ds_mut()?;
            ds.vector_edit = Some((id, vec![]));
            Ok(json!({"ok": true}))
        }),
        spec!("vector.edit.cornerRadius", "Point corner radius", "{radius, vertices?}", true, |s, p| {
            let (id, sel) = editing(s)?;
            let r = req_num("vector.edit.cornerRadius", p, "radius")?.max(0.0);
            if let Some(net) = network_mut(s.doc_mut()?, id) {
                for i in sel {
                    if let Some(v) = net.vertices.get_mut(i) {
                        v.radius = r;
                    }
                }
            }
            Ok(json!({"ok": true}))
        }),
    ]
}
