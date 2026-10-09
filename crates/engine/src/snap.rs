//! Snapping and smart guides: while moving or resizing, edges and centres snap to nearby layers'
//! edges/centres, the parent frame, layout grid columns and page guides; the matches come back as
//! guide lines (with spacing labels) for the canvas to draw in red.

use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_geom::Rect;
use serde::Serialize;

/// A guide line to draw: from (x0, y0) to (x1, y1) in page coordinates, with an optional label
/// (a distance) at its middle.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Guide {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub label: Option<String>,
}

/// Snap candidates gathered once at the start of a gesture.
#[derive(Clone, Debug, Default)]
pub struct Targets {
    /// Vertical lines (x) with the y-span of the rect they came from.
    pub xs: Vec<(f64, f64, f64)>,
    /// Horizontal lines (y) with the x-span.
    pub ys: Vec<(f64, f64, f64)>,
    /// Sibling boxes (for spacing labels).
    pub boxes: Vec<Rect>,
}

impl Targets {
    /// Gather snap lines from the siblings of `moving` (and the parent frame), page guides and
    /// layout grids, excluding the moving nodes and their descendants.
    pub fn gather(doc: &Document, page: NodeId, moving: &[NodeId]) -> Targets {
        let mut t = Targets::default();
        let push = |r: Rect, t: &mut Targets| {
            for x in [r.x0, r.center().x, r.x1] {
                t.xs.push((x, r.y0, r.y1));
            }
            for y in [r.y0, r.center().y, r.y1] {
                t.ys.push((y, r.x0, r.x1));
            }
            t.boxes.push(r);
        };
        let parent = moving.first().and_then(|m| doc.node(*m)).and_then(|n| n.parent).unwrap_or(page);
        // The parent frame's own box (and its layout grids).
        if parent != page
            && let Some(b) = doc.world_bounds(parent)
        {
            for x in [b.x0, b.center().x, b.x1] {
                t.xs.push((x, b.y0, b.y1));
            }
            for y in [b.y0, b.center().y, b.y1] {
                t.ys.push((y, b.x0, b.x1));
            }
            let (gx, gy) = figcraft_layout::grid_snap_lines(doc, parent);
            for x in gx {
                t.xs.push((x, b.y0, b.y1));
            }
            for y in gy {
                t.ys.push((y, b.x0, b.x1));
            }
        }
        // Siblings and, at the top level, all top-level layers.
        if let Some(p) = doc.node(parent) {
            for c in &p.children {
                if moving.contains(c) || !doc.node(*c).is_some_and(|n| n.visible) {
                    continue;
                }
                if let Some(b) = doc.world_bounds(*c) {
                    push(b, &mut t);
                }
            }
        }
        if let Some(NodeKind::Page { guides, .. }) = doc.node(page).map(|n| &n.kind) {
            for g in guides {
                if g.vertical {
                    t.xs.push((g.offset, -1e7, 1e7));
                } else {
                    t.ys.push((g.offset, -1e7, 1e7));
                }
            }
        }
        t
    }

    /// Snap a moving box: returns the (dx, dy) correction and guides to draw.
    pub fn snap_rect(&self, r: Rect, threshold: f64) -> (f64, f64, Vec<Guide>) {
        let xs = [r.x0, r.center().x, r.x1];
        let ys = [r.y0, r.center().y, r.y1];
        let best = |cands: &[(f64, f64, f64)], vals: &[f64]| -> Option<f64> {
            let mut best: Option<f64> = None;
            for (line, _, _) in cands {
                for v in vals {
                    let d = line - v;
                    if d.abs() <= threshold && best.is_none_or(|b| d.abs() < b.abs()) {
                        best = Some(d);
                    }
                }
            }
            best
        };
        let dx = best(&self.xs, &xs).unwrap_or(0.0);
        let dy = best(&self.ys, &ys).unwrap_or(0.0);
        let snapped = Rect::new(r.x0 + dx, r.y0 + dy, r.x1 + dx, r.y1 + dy);
        let guides = self.guides_for(snapped);
        (dx, dy, guides)
    }

    /// Snap a single point (resize handles, drawing).
    pub fn snap_point(&self, x: f64, y: f64, threshold: f64) -> (f64, f64, Vec<Guide>) {
        let mut bx: Option<f64> = None;
        for (line, _, _) in &self.xs {
            let d = line - x;
            if d.abs() <= threshold && bx.is_none_or(|b| d.abs() < b.abs()) {
                bx = Some(d);
            }
        }
        let mut by: Option<f64> = None;
        for (line, _, _) in &self.ys {
            let d = line - y;
            if d.abs() <= threshold && by.is_none_or(|b| d.abs() < b.abs()) {
                by = Some(d);
            }
        }
        let (nx, ny) = (x + bx.unwrap_or(0.0), y + by.unwrap_or(0.0));
        let mut g = Vec::new();
        if bx.is_some() {
            let span = self.xs.iter().filter(|(l, _, _)| (l - nx).abs() < 1e-6).fold((ny, ny), |a, (_, y0, y1)| (a.0.min(*y0), a.1.max(*y1)));
            g.push(Guide { x0: nx, y0: span.0, x1: nx, y1: span.1, label: None });
        }
        if by.is_some() {
            let span = self.ys.iter().filter(|(l, _, _)| (l - ny).abs() < 1e-6).fold((nx, nx), |a, (_, x0, x1)| (a.0.min(*x0), a.1.max(*x1)));
            g.push(Guide { x0: span.0, y0: ny, x1: span.1, y1: ny, label: None });
        }
        (bx.unwrap_or(0.0), by.unwrap_or(0.0), g)
    }

    /// Guide lines for edges of `r` that coincide with targets.
    pub fn guides_for(&self, r: Rect) -> Vec<Guide> {
        let mut out = Vec::new();
        for x in [r.x0, r.center().x, r.x1] {
            let hits: Vec<&(f64, f64, f64)> = self.xs.iter().filter(|(l, _, _)| (l - x).abs() < 1e-6).collect();
            if !hits.is_empty() {
                let y0 = hits.iter().map(|h| h.1).fold(r.y0, f64::min).max(-1e6);
                let y1 = hits.iter().map(|h| h.2).fold(r.y1, f64::max).min(1e6);
                out.push(Guide { x0: x, y0, x1: x, y1, label: None });
            }
        }
        for y in [r.y0, r.center().y, r.y1] {
            let hits: Vec<&(f64, f64, f64)> = self.ys.iter().filter(|(l, _, _)| (l - y).abs() < 1e-6).collect();
            if !hits.is_empty() {
                let x0 = hits.iter().map(|h| h.1).fold(r.x0, f64::min).max(-1e6);
                let x1 = hits.iter().map(|h| h.2).fold(r.x1, f64::max).min(1e6);
                out.push(Guide { x0, y0: y, x1, y1: y, label: None });
            }
        }
        out
    }
}

/// Distance lines from `r` to the nearest box on each side (Alt-hover measuring), or to `other`.
pub fn measure(r: Rect, other: Rect) -> Vec<Guide> {
    let mut out = Vec::new();
    let fmt = |d: f64| {
        let v = (d * 100.0).round() / 100.0;
        if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") }
    };
    if other.contains(r.origin()) && other.contains(figcraft_geom::Point::new(r.x1, r.y1)) {
        // Inside: distances to each edge of the container.
        let cy = r.center().y;
        let cx = r.center().x;
        out.push(Guide { x0: other.x0, y0: cy, x1: r.x0, y1: cy, label: Some(fmt(r.x0 - other.x0)) });
        out.push(Guide { x0: r.x1, y0: cy, x1: other.x1, y1: cy, label: Some(fmt(other.x1 - r.x1)) });
        out.push(Guide { x0: cx, y0: other.y0, x1: cx, y1: r.y0, label: Some(fmt(r.y0 - other.y0)) });
        out.push(Guide { x0: cx, y0: r.y1, x1: cx, y1: other.y1, label: Some(fmt(other.y1 - r.y1)) });
        return out;
    }
    let cy = (r.y0.max(other.y0) + r.y1.min(other.y1)) / 2.0;
    let cx = (r.x0.max(other.x0) + r.x1.min(other.x1)) / 2.0;
    let cy = if r.y0.max(other.y0) <= r.y1.min(other.y1) { cy } else { r.center().y };
    let cx = if r.x0.max(other.x0) <= r.x1.min(other.x1) { cx } else { r.center().x };
    if other.x0 >= r.x1 {
        out.push(Guide { x0: r.x1, y0: cy, x1: other.x0, y1: cy, label: Some(fmt(other.x0 - r.x1)) });
    } else if other.x1 <= r.x0 {
        out.push(Guide { x0: other.x1, y0: cy, x1: r.x0, y1: cy, label: Some(fmt(r.x0 - other.x1)) });
    }
    if other.y0 >= r.y1 {
        out.push(Guide { x0: cx, y0: r.y1, x1: cx, y1: other.y0, label: Some(fmt(other.y0 - r.y1)) });
    } else if other.y1 <= r.y0 {
        out.push(Guide { x0: cx, y0: other.y1, x1: cx, y1: r.y0, label: Some(fmt(r.y0 - other.y1)) });
    }
    out
}
