//! FigCraft layout.
//!
//! - [`layout_page`] / [`layout_frame`]: Figma-style auto layout. Children of an auto layout frame
//!   are stacked horizontally or vertically (optionally wrapping) or placed on a grid, with gap,
//!   padding and alignment; each child is Fixed, Hug (its own content) or Fill (shares the free
//!   space). A Hug container sizes to its children. Absolute children are left alone.
//! - [`apply_constraints`]: children of a regular frame follow their constraints when it resizes.
//! - [`grid_lines`]: the column/row rectangles of a layout grid, for drawing and snapping.
//!
//! Text measurement is injected ([`Measure`]) so this crate doesn't depend on fonts.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use figcraft_doc::{
    AutoLayout, CrossAlign, Document, GridAlign, GridKind, HConstraint, LayoutGrid, LayoutMode, MainAlign, NodeId, NodeKind, Sizing, TextAutoResize,
    VConstraint,
};
use figcraft_geom::{Affine, Point, Rect};

/// Measures text for a given width (auto-height text inside a Fill container).
pub trait Measure {
    /// The height a text node needs at `width`.
    fn text_height(&self, doc: &Document, id: NodeId, width: f64) -> Option<f64>;
    /// Natural (auto width) size of a text node.
    fn text_size(&self, doc: &Document, id: NodeId) -> Option<(f64, f64)>;
}

/// A measurer that never changes text sizes.
pub struct NoMeasure;

impl Measure for NoMeasure {
    fn text_height(&self, _: &Document, _: NodeId, _: f64) -> Option<f64> {
        None
    }
    fn text_size(&self, _: &Document, _: NodeId) -> Option<(f64, f64)> {
        None
    }
}

fn clamp_size(v: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let mut v = if v.is_finite() { v.max(0.0) } else { 0.0 };
    if let Some(m) = max.filter(|m| m.is_finite()) {
        v = v.min(m);
    }
    if let Some(m) = min.filter(|m| m.is_finite()) {
        v = v.max(m);
    }
    v
}

/// The child's box size as seen by the parent (rotation → bounding box).
fn outer_size(doc: &Document, id: NodeId) -> (f64, f64) {
    let Some(n) = doc.node(id) else { return (0.0, 0.0) };
    let b = figcraft_geom::transform_rect_bbox(linear(n.transform), n.local_rect());
    (b.width(), b.height())
}

fn linear(a: Affine) -> Affine {
    let c = a.as_coeffs();
    Affine::new([c[0], c[1], c[2], c[3], 0.0, 0.0])
}

/// Place `id` so its (rotated) bounding box's top-left is at `(x, y)` in the parent.
fn place(doc: &mut Document, id: NodeId, x: f64, y: f64) {
    let Some(n) = doc.node(id) else { return };
    let lin = linear(n.transform);
    let b = figcraft_geom::transform_rect_bbox(lin, n.local_rect());
    let t = Affine::translate((x - b.x0, y - b.y0)) * lin;
    if let Some(n) = doc.node_mut(id) {
        n.transform = t;
    }
}

/// Resize a node's own box (vectors scale their network to stay inside it).
pub fn set_size(doc: &mut Document, id: NodeId, w: f64, h: f64) {
    let Some(n) = doc.node_mut(id) else { return };
    let (w, h) = (if w.is_finite() { w.max(0.0) } else { n.width }, if h.is_finite() { h.max(0.0) } else { n.height });
    let (ow, oh) = (n.width, n.height);
    if (ow - w).abs() < 1e-9 && (oh - h).abs() < 1e-9 {
        return;
    }
    if let NodeKind::Vector { network } = &mut n.kind {
        let sx = if ow > 1e-9 { w / ow } else { 1.0 };
        let sy = if oh > 1e-9 { h / oh } else { 1.0 };
        network.scale(sx, sy);
    }
    n.width = w;
    n.height = h;
}

/// Lay out every auto layout frame on a page (children before parents).
pub fn layout_page(doc: &mut Document, page: NodeId, m: &dyn Measure) {
    let order = doc.subtree(page);
    for id in order.into_iter().rev() {
        if doc.node(id).is_some_and(|n| n.layout.is_some() && n.kind.is_frame_like()) {
            layout_frame(doc, id, m);
        }
    }
}

/// Lay out the ancestors of `id` that are auto layout frames (innermost first), and `id` itself.
pub fn layout_up(doc: &mut Document, id: NodeId, m: &dyn Measure) {
    let mut chain = vec![id];
    chain.extend(doc.ancestors(id));
    for c in chain {
        if doc.node(c).is_some_and(|n| n.layout.is_some() && n.kind.is_frame_like()) {
            layout_frame(doc, c, m);
        }
    }
}

struct Item {
    id: NodeId,
    main: f64,
    cross: f64,
    fill_main: bool,
    fill_cross: bool,
    min_main: Option<f64>,
    max_main: Option<f64>,
    min_cross: Option<f64>,
    max_cross: Option<f64>,
}

/// Lay out one auto layout frame's children and size the frame if it hugs.
pub fn layout_frame(doc: &mut Document, frame: NodeId, m: &dyn Measure) {
    let Some(f) = doc.node(frame) else { return };
    let Some(al) = f.layout.clone() else { return };
    let horizontal = matches!(al.mode, LayoutMode::Horizontal);
    let (hug_w, hug_h) = (f.child_layout.sizing_h == Sizing::Hug, f.child_layout.sizing_v == Sizing::Hug);
    let (fw, fh) = (f.width, f.height);
    let stroke_pad = if al.strokes_included && f.has_visible_stroke() { f.stroke.weight } else { 0.0 };
    let [pt, pr, pb, pl] = al.padding.map(|p| if p.is_finite() { p.max(0.0) } else { 0.0 });
    let (pt, pr, pb, pl) = (pt + stroke_pad, pr + stroke_pad, pb + stroke_pad, pl + stroke_pad);
    let kids: Vec<NodeId> = f.children.iter().copied().filter(|c| doc.node(*c).is_some_and(|n| n.visible && !n.child_layout.absolute)).collect();

    // Text children: refresh their natural size first.
    for c in &kids {
        if let Some(n) = doc.node(*c)
            && let NodeKind::Text(t) = &n.kind
            && t.auto_resize == TextAutoResize::WidthAndHeight
            && let Some((w, h)) = m.text_size(doc, *c)
        {
            set_size(doc, *c, w, h);
        }
    }

    if matches!(al.mode, LayoutMode::Grid) {
        layout_grid(doc, frame, &al, &kids, (pt, pr, pb, pl), m);
        return;
    }

    let inner_main_fixed = if horizontal { fw - pl - pr } else { fh - pt - pb };
    let inner_cross_fixed = if horizontal { fh - pt - pb } else { fw - pl - pr };
    let hug_main = if horizontal { hug_w } else { hug_h };
    let hug_cross = if horizontal { hug_h } else { hug_w };

    let mut items: Vec<Item> = Vec::with_capacity(kids.len());
    for c in &kids {
        let Some(n) = doc.node(*c) else { continue };
        let (w, h) = outer_size(doc, *c);
        let cl = &n.child_layout;
        let (sm, sc) = if horizontal { (cl.sizing_h, cl.sizing_v) } else { (cl.sizing_v, cl.sizing_h) };
        let (min_main, max_main, min_cross, max_cross) =
            if horizontal { (cl.min_w, cl.max_w, cl.min_h, cl.max_h) } else { (cl.min_h, cl.max_h, cl.min_w, cl.max_w) };
        items.push(Item {
            id: *c,
            main: if horizontal { w } else { h },
            cross: if horizontal { h } else { w },
            fill_main: sm == Sizing::Fill && !hug_main,
            fill_cross: sc == Sizing::Fill && !hug_cross,
            min_main,
            max_main,
            min_cross,
            max_cross,
        });
    }

    let gap = if al.gap.is_finite() { al.gap } else { 0.0 };
    let between = al.main_align == MainAlign::SpaceBetween;

    // Split into lines (wrap only on the main axis of a horizontal layout, like Figma).
    let mut lines: Vec<Vec<usize>> = Vec::new();
    if al.wrap && !hug_main {
        let mut cur: Vec<usize> = Vec::new();
        let mut used = 0.0;
        for (i, it) in items.iter().enumerate() {
            let sz = if it.fill_main { it.min_main.unwrap_or(0.0) } else { it.main };
            let add = if cur.is_empty() { sz } else { gap.max(0.0) + sz };
            if !cur.is_empty() && used + add > inner_main_fixed + 1e-6 {
                lines.push(std::mem::take(&mut cur));
                used = sz;
            } else {
                used += add;
            }
            cur.push(i);
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
    } else {
        lines.push((0..items.len()).collect());
    }

    // Resolve main sizes per line.
    let mut line_extents: Vec<(f64, f64)> = Vec::new(); // (main used, cross size)
    for line in &lines {
        let fixed: f64 = line.iter().filter_map(|i| items.get(*i)).filter(|it| !it.fill_main).map(|it| it.main).sum();
        let gaps = gap * line.len().saturating_sub(1) as f64;
        let fills: Vec<usize> = line.iter().copied().filter(|i| items.get(*i).is_some_and(|it| it.fill_main)).collect();
        if !fills.is_empty() {
            let mut free = (inner_main_fixed - fixed - if between { 0.0 } else { gaps }).max(0.0);
            let mut open: Vec<usize> = fills.clone();
            // Distribute, honouring min/max (a few rounds).
            for _ in 0..4 {
                if open.is_empty() {
                    break;
                }
                let share = free / open.len() as f64;
                let mut still = Vec::new();
                let mut used = 0.0;
                for i in &open {
                    let Some(it) = items.get_mut(*i) else { continue };
                    let v = clamp_size(share, it.min_main, it.max_main);
                    it.main = v;
                    if (v - share).abs() > 1e-9 {
                        used += v;
                    } else {
                        still.push(*i);
                    }
                }
                if still.len() == open.len() {
                    break;
                }
                free = (free - used).max(0.0);
                open = still;
            }
        }
        let main_used: f64 = line.iter().filter_map(|i| items.get(*i)).map(|it| it.main).sum::<f64>() + gaps;
        let cross: f64 = line.iter().filter_map(|i| items.get(*i)).filter(|it| !it.fill_cross).map(|it| it.cross).fold(0.0, f64::max);
        line_extents.push((main_used, cross));
    }

    // Container size.
    let content_main = line_extents.iter().map(|l| l.0).fold(0.0, f64::max);
    let cross_gap = if al.cross_gap.is_finite() { al.cross_gap } else { 0.0 };
    let content_cross: f64 = line_extents.iter().map(|l| l.1).sum::<f64>() + cross_gap * lines.len().saturating_sub(1) as f64;
    let (mut nw, mut nh) = (fw, fh);
    let fcl = doc.node(frame).map(|n| n.child_layout.clone()).unwrap_or_default();
    if hug_main {
        let v = content_main + if horizontal { pl + pr } else { pt + pb };
        if horizontal {
            nw = clamp_size(v, fcl.min_w, fcl.max_w);
        } else {
            nh = clamp_size(v, fcl.min_h, fcl.max_h);
        }
    }
    if hug_cross {
        // Fill-cross children inside a hugging container use their own size.
        let max_fill_cross = items.iter().filter(|it| it.fill_cross).map(|it| it.cross).fold(0.0, f64::max);
        let cc = if lines.len() <= 1 { content_cross.max(max_fill_cross) } else { content_cross };
        let v = cc + if horizontal { pt + pb } else { pl + pr };
        if horizontal {
            nh = clamp_size(v, fcl.min_h, fcl.max_h);
        } else {
            nw = clamp_size(v, fcl.min_w, fcl.max_w);
        }
    }
    set_size(doc, frame, nw, nh);
    let inner_main = if horizontal { nw - pl - pr } else { nh - pt - pb };
    let inner_cross = if horizontal { nh - pt - pb } else { nw - pl - pr };
    let _ = inner_cross_fixed;

    // Position.
    let mut cross_pos = if horizontal { pt } else { pl };
    let single = lines.len() == 1;
    for (li, line) in lines.iter().enumerate() {
        let (used, line_cross) = line_extents.get(li).copied().unwrap_or((0.0, 0.0));
        let line_cross = if single { inner_cross } else { line_cross };
        let n_items = line.len();
        let (mut pos, step_gap) = match al.main_align {
            MainAlign::Min => (0.0, gap),
            MainAlign::Center => ((inner_main - used) / 2.0, gap),
            MainAlign::Max => (inner_main - used, gap),
            MainAlign::SpaceBetween => {
                let total: f64 = line.iter().filter_map(|i| items.get(*i)).map(|it| it.main).sum();
                if n_items > 1 { (0.0, (inner_main - total) / (n_items - 1) as f64) } else { ((inner_main - total) / 2.0, 0.0) }
            }
        };
        pos += if horizontal { pl } else { pt };
        for i in line {
            let Some(it) = items.get(*i) else { continue };
            let cross_size = if it.fill_cross { clamp_size(line_cross, it.min_cross, it.max_cross) } else { it.cross };
            let off = match al.cross_align {
                CrossAlign::Min | CrossAlign::Baseline => 0.0,
                CrossAlign::Center => (line_cross - cross_size) / 2.0,
                CrossAlign::Max => line_cross - cross_size,
            };
            let (w, h) = if horizontal { (it.main, cross_size) } else { (cross_size, it.main) };
            // Resize only when the layout decides the size (fill, or text that must re-wrap).
            if it.fill_main || it.fill_cross {
                resize_child(doc, it.id, w, h, m);
            }
            let (x, y) = if horizontal { (pos, cross_pos + off) } else { (cross_pos + off, pos) };
            place(doc, it.id, x, y);
            pos += it.main + step_gap;
        }
        cross_pos += line_cross + cross_gap;
    }
    // Hugging after fill-cross text re-wrapped: heights may have changed; one more pass is cheap.
    let _ = AutoLayout::default();
}

fn resize_child(doc: &mut Document, id: NodeId, w: f64, h: f64, m: &dyn Measure) {
    let is_text_height = doc
        .node(id)
        .is_some_and(|n| matches!(&n.kind, NodeKind::Text(t) if t.auto_resize != TextAutoResize::None && t.auto_resize != TextAutoResize::Truncate));
    if is_text_height {
        if let Some(n) = doc.node_mut(id)
            && let NodeKind::Text(t) = &mut n.kind
            && t.auto_resize == TextAutoResize::WidthAndHeight
        {
            t.auto_resize = TextAutoResize::Height;
        }
        let th = m.text_height(doc, id, w).unwrap_or(h);
        set_size(doc, id, w, th);
    } else {
        let old = doc.node(id).map(|n| (n.width, n.height));
        set_size(doc, id, w, h);
        if let Some((ow, oh)) = old
            && doc.node(id).is_some_and(|n| n.layout.is_none() && n.kind.is_frame_like())
        {
            apply_constraints(doc, id, ow, oh);
        } else if doc.node(id).is_some_and(|n| n.layout.is_some()) {
            layout_frame(doc, id, m);
        }
    }
}

fn layout_grid(doc: &mut Document, frame: NodeId, al: &AutoLayout, kids: &[NodeId], pad: (f64, f64, f64, f64), m: &dyn Measure) {
    let (pt, pr, pb, pl) = pad;
    let Some(f) = doc.node(frame) else { return };
    let cols = al.grid_columns.clamp(1, 1000) as usize;
    let rows_needed = kids.len().div_ceil(cols).max(al.grid_rows.clamp(1, 1000) as usize);
    let (fw, fh) = (f.width, f.height);
    let gx = if al.gap.is_finite() { al.gap.max(0.0) } else { 0.0 };
    let gy = if al.cross_gap.is_finite() { al.cross_gap.max(0.0) } else { 0.0 };
    let hug_w = f.child_layout.sizing_h == Sizing::Hug;
    let hug_h = f.child_layout.sizing_v == Sizing::Hug;
    // Track sizes: equal (1fr) unless hugging, then the max child per track.
    let mut col_w = vec![0.0f64; cols];
    let mut row_h = vec![0.0f64; rows_needed];
    for (i, c) in kids.iter().enumerate() {
        let (w, h) = outer_size(doc, *c);
        if let Some(cw) = col_w.get_mut(i % cols) {
            *cw = cw.max(w);
        }
        if let Some(rh) = row_h.get_mut(i / cols) {
            *rh = rh.max(h);
        }
    }
    let total_w = if hug_w { col_w.iter().sum::<f64>() + gx * (cols - 1) as f64 + pl + pr } else { fw };
    let total_h = if hug_h { row_h.iter().sum::<f64>() + gy * (rows_needed - 1) as f64 + pt + pb } else { fh };
    set_size(doc, frame, total_w, total_h);
    if !hug_w {
        let cw = ((total_w - pl - pr - gx * (cols - 1) as f64) / cols as f64).max(0.0);
        col_w = vec![cw; cols];
    }
    if !hug_h {
        let rh = ((total_h - pt - pb - gy * (rows_needed - 1) as f64) / rows_needed as f64).max(0.0);
        row_h = vec![rh; rows_needed];
    }
    for (i, c) in kids.iter().enumerate() {
        let (ci, ri) = (i % cols, i / cols);
        let x = pl + col_w.iter().take(ci).sum::<f64>() + gx * ci as f64;
        let y = pt + row_h.iter().take(ri).sum::<f64>() + gy * ri as f64;
        let (cw, rh) = (col_w.get(ci).copied().unwrap_or(0.0), row_h.get(ri).copied().unwrap_or(0.0));
        let Some(n) = doc.node(*c) else { continue };
        let (fill_w, fill_h) = (n.child_layout.sizing_h == Sizing::Fill, n.child_layout.sizing_v == Sizing::Fill);
        let (w, h) = outer_size(doc, *c);
        let (nw, nh) = (if fill_w { cw } else { w }, if fill_h { rh } else { h });
        if fill_w || fill_h {
            resize_child(doc, *c, nw, nh, m);
        }
        let ox = match al.main_align {
            MainAlign::Center => (cw - nw) / 2.0,
            MainAlign::Max => cw - nw,
            _ => 0.0,
        };
        let oy = match al.cross_align {
            CrossAlign::Center => (rh - nh) / 2.0,
            CrossAlign::Max => rh - nh,
            _ => 0.0,
        };
        place(doc, *c, x + ox, y + oy);
    }
}

/// After frame `id` (without auto layout) changed size from `(ow, oh)`, move/resize its children
/// per their constraints (recursively for children frames that resize).
pub fn apply_constraints(doc: &mut Document, id: NodeId, ow: f64, oh: f64) {
    let Some(f) = doc.node(id) else { return };
    if f.layout.is_some() {
        return;
    }
    let (nw, nh) = (f.width, f.height);
    if (nw - ow).abs() < 1e-9 && (nh - oh).abs() < 1e-9 {
        return;
    }
    let sx = if ow > 1e-9 { nw / ow } else { 1.0 };
    let sy = if oh > 1e-9 { nh / oh } else { 1.0 };
    let kids = f.children.clone();
    // Group children follow the group's own bounds; constraints apply to frames' children.
    if f.kind.bounds_follow_children() {
        return;
    }
    for c in kids {
        let Some(n) = doc.node(c) else { continue };
        let lin = linear(n.transform);
        let b = figcraft_geom::transform_rect_bbox(n.transform, n.local_rect());
        let (cw, ch) = (n.width, n.height);
        let (bw, bh) = (b.width(), b.height());
        let (mut x0, mut x1) = (b.x0, b.x1);
        let (mut y0, mut y1) = (b.y0, b.y1);
        match n.constraints.horizontal {
            HConstraint::Left => {}
            HConstraint::Right => {
                x0 += nw - ow;
                x1 += nw - ow;
            }
            HConstraint::LeftRight => x1 += nw - ow,
            HConstraint::Center => {
                let d = (nw - ow) / 2.0;
                x0 += d;
                x1 += d;
            }
            HConstraint::Scale => {
                x0 *= sx;
                x1 *= sx;
            }
        }
        match n.constraints.vertical {
            VConstraint::Top => {}
            VConstraint::Bottom => {
                y0 += nh - oh;
                y1 += nh - oh;
            }
            VConstraint::TopBottom => y1 += nh - oh,
            VConstraint::Center => {
                let d = (nh - oh) / 2.0;
                y0 += d;
                y1 += d;
            }
            VConstraint::Scale => {
                y0 *= sy;
                y1 *= sy;
            }
        }
        // New local size from the bbox change (axis-aligned children exactly; rotated approximately).
        let kw = if bw > 1e-9 { (x1 - x0) / bw } else { 1.0 };
        let kh = if bh > 1e-9 { (y1 - y0) / bh } else { 1.0 };
        let (new_w, new_h) = (cw * kw, ch * kh);
        let resized = (new_w - cw).abs() > 1e-9 || (new_h - ch).abs() > 1e-9;
        if resized {
            set_size(doc, c, new_w, new_h);
            if let Some(cn) = doc.node_mut(c) {
                cn.transform = lin;
            }
        }
        place(doc, c, x0, y0);
        if resized && doc.node(c).is_some_and(|n| n.kind.is_frame_like() || n.kind.bounds_follow_children()) {
            if doc.node(c).is_some_and(|n| n.kind.bounds_follow_children()) {
                scale_group_children(doc, c, kw, kh);
            } else {
                apply_constraints(doc, c, cw, ch);
            }
        }
    }
}

/// Scale a group's children when the group is resized (groups scale their contents).
pub fn scale_group_children(doc: &mut Document, id: NodeId, kx: f64, ky: f64) {
    let Some(n) = doc.node(id) else { return };
    for c in n.children.clone() {
        let Some(cn) = doc.node(c) else { continue };
        let b = figcraft_geom::transform_rect_bbox(cn.transform, cn.local_rect());
        let (w, h) = (cn.width, cn.height);
        set_size(doc, c, w * kx, h * ky);
        place(doc, c, b.x0 * kx, b.y0 * ky);
        if doc.node(c).is_some_and(|n| n.kind.bounds_follow_children()) {
            scale_group_children(doc, c, kx, ky);
        } else if doc.node(c).is_some_and(|n| n.kind.is_frame_like()) {
            apply_constraints(doc, c, w, h);
        }
    }
}

/// The rectangles (frame-local) a layout grid draws: columns, rows or the square grid's lines.
pub fn grid_lines(g: &LayoutGrid, w: f64, h: f64) -> Vec<Rect> {
    let mut out = Vec::new();
    match g.kind {
        GridKind::Grid => {
            let s = if g.size.is_finite() && g.size >= 1.0 { g.size } else { 10.0 };
            let mut x = 0.0;
            let mut guard = 0;
            while x <= w && guard < 4000 {
                out.push(Rect::new(x, 0.0, x, h));
                x += s;
                guard += 1;
            }
            let mut y = 0.0;
            guard = 0;
            while y <= h && guard < 4000 {
                out.push(Rect::new(0.0, y, w, y));
                y += s;
                guard += 1;
            }
        }
        GridKind::Columns | GridKind::Rows => {
            let cols = g.kind == GridKind::Columns;
            let total = if cols { w } else { h };
            let count = g.count.clamp(1, 500) as usize;
            let gutter = if g.gutter.is_finite() { g.gutter.max(0.0) } else { 0.0 };
            let offset = if g.offset.is_finite() { g.offset.max(0.0) } else { 0.0 };
            let (start, size) = match g.align {
                GridAlign::Stretch => {
                    let size = ((total - 2.0 * offset - gutter * (count - 1) as f64) / count as f64).max(0.0);
                    (offset, size)
                }
                _ => {
                    let size = if g.size.is_finite() && g.size > 0.0 { g.size } else { 60.0 };
                    let span = size * count as f64 + gutter * (count - 1) as f64;
                    let start = match g.align {
                        GridAlign::Min => offset,
                        GridAlign::Center => (total - span) / 2.0,
                        _ => total - offset - span,
                    };
                    (start, size)
                }
            };
            for i in 0..count {
                let a = start + i as f64 * (size + gutter);
                out.push(if cols { Rect::new(a, 0.0, a + size, h) } else { Rect::new(0.0, a, w, a + size) });
            }
        }
    }
    out
}

/// Snap targets of a frame's visible layout grids (page coordinates): column/row edges.
pub fn grid_snap_lines(doc: &Document, frame: NodeId) -> (Vec<f64>, Vec<f64>) {
    let Some(f) = doc.node(frame) else { return (vec![], vec![]) };
    let xf = doc.world_transform(frame);
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for g in f.grids.iter().filter(|g| g.visible) {
        for r in grid_lines(g, f.width, f.height) {
            let a = xf * Point::new(r.x0, r.y0);
            let b = xf * Point::new(r.x1, r.y1);
            match g.kind {
                GridKind::Columns => {
                    xs.push(a.x);
                    xs.push(b.x);
                }
                GridKind::Rows => {
                    ys.push(a.y);
                    ys.push(b.y);
                }
                GridKind::Grid => {
                    if (r.x0 - r.x1).abs() < 1e-9 {
                        xs.push(a.x);
                    } else {
                        ys.push(a.y);
                    }
                }
            }
        }
    }
    (xs, ys)
}

#[cfg(test)]
mod tests;
