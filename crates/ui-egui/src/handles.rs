//! On-canvas editing handles that sit above the pointer tools: gradient handles (while a
//! gradient fill is open in the colour picker) and auto layout padding/gap handles.

use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use figcraft_color::{Paint, PaintKind};
use figcraft_doc::{LayoutMode, NodeId};
use figcraft_geom::Point;
use serde_json::json;

use crate::FigApp;
use crate::colorpicker::PickerTarget;
use crate::theme::{ACCENT, semibold};

const PINK: Color32 = Color32::from_rgb(0xF2, 0x48, 0x8C);

#[derive(Clone, Debug)]
enum Drag {
    /// Gradient handle 0 = from, 1 = to, 2 = width.
    Gradient {
        id: NodeId,
        fills: bool,
        index: usize,
        which: usize,
    },
    /// Padding side 0 top, 1 right, 2 bottom, 3 left.
    Padding {
        id: NodeId,
        side: usize,
        start: Pos2,
        value: f64,
    },
    Gap {
        id: NodeId,
        start: Pos2,
        value: f64,
        vertical: bool,
    },
}

fn drag_id() -> Id {
    Id::new("figcraft_handle_drag")
}

/// The gradient being edited (single node, picker open on a gradient paint).
fn gradient_target(app: &FigApp) -> Option<(NodeId, bool, usize, figcraft_color::Gradient)> {
    let Some(PickerTarget::Paint { fills, index, ids }) = &app.ui.picker else { return None };
    let id = *ids.first()?;
    if ids.len() != 1 {
        return None;
    }
    let n = app.session.doc().ok()?.node(id)?;
    let list = if *fills { &n.fills } else { &n.strokes };
    match &list.get(*index)?.kind {
        PaintKind::Gradient(g) => Some((id, *fills, *index, g.clone())),
        _ => None,
    }
}

fn unit_to_screen(app: &FigApp, id: NodeId, u: (f64, f64)) -> Option<Pos2> {
    let doc = app.session.doc().ok()?;
    let n = doc.node(id)?;
    let p = doc.world_transform(id) * Point::new(u.0 * n.width, u.1 * n.height);
    Some(app.to_screen(p.x, p.y))
}

fn screen_to_unit(app: &FigApp, id: NodeId, s: Pos2) -> Option<(f64, f64)> {
    let doc = app.session.doc().ok()?;
    let n = doc.node(id)?;
    let p = app.to_page(s);
    let l = doc.world_transform(id).inverse() * p;
    Some((l.x / n.width.max(1e-9), l.y / n.height.max(1e-9)))
}

/// Auto layout zones of a single selected AL frame, in screen space: padding bands and gaps.
fn al_zones(app: &FigApp) -> Option<(NodeId, Vec<(usize, Rect, f64)>, Vec<Rect>, f64, bool)> {
    let ds = app.session.ds().ok()?;
    if ds.selection.len() != 1 || ds.text_edit.is_some() || ds.vector_edit.is_some() {
        return None;
    }
    let id = *ds.selection.first()?;
    let doc = &ds.doc;
    let n = doc.node(id)?;
    let al = n.layout.as_ref()?;
    // Only for unrotated frames (axis-aligned bands).
    let w = doc.world_transform(id);
    if figcraft_geom::rotation_degrees(w).abs() > 0.01 {
        return None;
    }
    let to = |x: f64, y: f64| {
        let p = w * Point::new(x, y);
        app.to_screen(p.x, p.y)
    };
    let [pt, pr, pb, pl] = al.padding;
    let (fw, fh) = (n.width, n.height);
    let bands = vec![
        (0, Rect::from_two_pos(to(0.0, 0.0), to(fw, pt.max(4.0))), pt),
        (1, Rect::from_two_pos(to(fw - pr.max(4.0), 0.0), to(fw, fh)), pr),
        (2, Rect::from_two_pos(to(0.0, fh - pb.max(4.0)), to(fw, fh)), pb),
        (3, Rect::from_two_pos(to(0.0, 0.0), to(pl.max(4.0), fh)), pl),
    ];
    let vertical = al.mode == LayoutMode::Vertical;
    let mut gaps = Vec::new();
    if al.mode != LayoutMode::Grid && !al.wrap {
        let kids: Vec<_> = n.children.iter().filter_map(|c| doc.node(*c)).filter(|c| c.visible && !c.child_layout.absolute).collect();
        for pair in kids.windows(2) {
            let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else { continue };
            let ra = figcraft_geom::transform_rect_bbox(a.transform, a.local_rect());
            let rb = figcraft_geom::transform_rect_bbox(b.transform, b.local_rect());
            let r =
                if vertical { Rect::from_two_pos(to(pl, ra.y1), to(fw - pr, rb.y0)) } else { Rect::from_two_pos(to(ra.x1, pt), to(rb.x0, fh - pb)) };
            if r.width() > 0.5 && r.height() > 0.5 {
                gaps.push(r);
            }
        }
    }
    Some((id, bands, gaps, al.gap, vertical))
}

/// Draw the handles.
pub fn draw(app: &FigApp, painter: &egui::Painter) {
    let hover = painter.ctx().input(|i| i.pointer.hover_pos());
    if let Some((id, _, _, g)) = gradient_target(app) {
        let (Some(a), Some(b), Some(c)) = (unit_to_screen(app, id, g.from), unit_to_screen(app, id, g.to), unit_to_screen(app, id, g.width)) else {
            return;
        };
        painter.line_segment([a, b], Stroke::new(2.0, Color32::WHITE));
        painter.line_segment([a, b], Stroke::new(1.0, Color32::from_black_alpha(120)));
        if g.kind != figcraft_color::GradientKind::Linear {
            painter.add(Shape::dashed_line(&[a, c], Stroke::new(1.0, Color32::WHITE), 4.0, 3.0));
        }
        for s in g.sorted_stops() {
            let p = a + (b - a) * s.position;
            let r = Rect::from_center_size(p, vec2(12.0, 12.0));
            painter.rect_filled(r, CornerRadius::same(2), crate::theme::c32(s.color.with_alpha(1.0)));
            painter.rect_stroke(r, CornerRadius::same(2), Stroke::new(2.0, Color32::WHITE), StrokeKind::Outside);
        }
        for (i, p) in [a, b, c].iter().enumerate() {
            if i == 2 && g.kind == figcraft_color::GradientKind::Linear {
                continue;
            }
            painter.circle_filled(*p, 5.0, Color32::WHITE);
            painter.circle_stroke(*p, 5.0, Stroke::new(1.5, ACCENT));
        }
        return;
    }
    let tool_move = app.session.tool.tool == figcraft_engine::ToolId::Move && !app.session.tool.busy();
    let dragging: Option<Drag> = painter.ctx().data(|d| d.get_temp(drag_id()));
    if !tool_move && dragging.is_none() {
        return;
    }
    let Some((_, bands, gaps, gap, _)) = al_zones(app) else { return };
    let active_side = match &dragging {
        Some(Drag::Padding { side, .. }) => Some(*side),
        _ => None,
    };
    for (side, r, v) in &bands {
        let hot = hover.is_some_and(|h| r.contains(h)) || active_side == Some(*side);
        if hot {
            painter.rect_filled(*r, CornerRadius::ZERO, PINK.gamma_multiply(0.25));
            let label = crate::canvas::fmt(*v);
            let galley = painter.layout_no_wrap(label, semibold(10.0), Color32::WHITE);
            let lr = Rect::from_center_size(r.center(), galley.size() + vec2(8.0, 4.0));
            painter.rect_filled(lr, CornerRadius::same(3), PINK);
            painter.galley(lr.min + vec2(4.0, 2.0), galley, Color32::WHITE);
        }
    }
    let gap_drag = matches!(dragging, Some(Drag::Gap { .. }));
    for r in &gaps {
        let hot = hover.is_some_and(|h| r.contains(h)) || gap_drag;
        if hot {
            painter.rect_filled(*r, CornerRadius::ZERO, PINK.gamma_multiply(0.3));
            let c = r.center();
            painter.line_segment([pos2(c.x - 3.0, c.y), pos2(c.x + 3.0, c.y)], Stroke::new(1.0, PINK));
            let galley = painter.layout_no_wrap(crate::canvas::fmt(gap), semibold(10.0), Color32::WHITE);
            let lr = Rect::from_center_size(c, galley.size() + vec2(8.0, 4.0));
            painter.rect_filled(lr, CornerRadius::same(3), PINK);
            painter.galley(lr.min + vec2(4.0, 2.0), galley, Color32::WHITE);
        }
    }
    let _ = Align2::CENTER_CENTER;
}

/// Pointer handling; returns true when a handle owns the pointer this frame.
pub fn input(app: &mut FigApp, ctx: &egui::Context, pos: Pos2) -> bool {
    let (pressed, down, released, shift) = ctx.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Primary),
            i.pointer.primary_down(),
            i.pointer.button_released(egui::PointerButton::Primary),
            i.modifiers.shift,
        )
    });
    let dragging: Option<Drag> = ctx.data(|d| d.get_temp(drag_id()));
    if let Some(drag) = dragging {
        if down {
            apply(app, &drag, pos, shift);
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if released || !down {
            let _ = app.session.commit_interaction();
            ctx.data_mut(|d| d.remove::<Drag>(drag_id()));
        }
        return true;
    }
    // Hover cursors / press to start.
    if let Some((id, fills, index, g)) = gradient_target(app) {
        let pts = [unit_to_screen(app, id, g.from), unit_to_screen(app, id, g.to), unit_to_screen(app, id, g.width)];
        for (which, p) in pts.iter().enumerate() {
            if which == 2 && g.kind == figcraft_color::GradientKind::Linear {
                continue;
            }
            if let Some(p) = p
                && (*p - pos).length() <= 8.0
            {
                ctx.set_cursor_icon(egui::CursorIcon::Grab);
                if pressed {
                    let _ = app.session.begin_interaction("Edit gradient");
                    ctx.data_mut(|d| d.insert_temp(drag_id(), Drag::Gradient { id, fills, index, which }));
                }
                return true;
            }
        }
        return false;
    }
    if app.session.tool.tool != figcraft_engine::ToolId::Move || app.session.tool.busy() {
        return false;
    }
    let Some((id, bands, gaps, gap, vertical)) = al_zones(app) else { return false };
    for (side, r, v) in bands {
        if r.contains(pos) {
            ctx.set_cursor_icon(if side % 2 == 0 { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
            if pressed {
                let _ = app.session.begin_interaction("Padding");
                ctx.data_mut(|d| d.insert_temp(drag_id(), Drag::Padding { id, side, start: pos, value: v }));
                return true;
            }
            return false;
        }
    }
    for r in gaps {
        if r.contains(pos) {
            ctx.set_cursor_icon(if vertical { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
            if pressed {
                let _ = app.session.begin_interaction("Gap");
                ctx.data_mut(|d| d.insert_temp(drag_id(), Drag::Gap { id, start: pos, value: gap, vertical }));
                return true;
            }
            return false;
        }
    }
    false
}

fn apply(app: &mut FigApp, drag: &Drag, pos: Pos2, shift: bool) {
    let zoom = app.session.ds().map(|d| d.view.zoom).unwrap_or(1.0).max(1e-6);
    match drag {
        Drag::Gradient { id, fills, index, which } => {
            let Some(u) = screen_to_unit(app, *id, pos) else { return };
            let Some(n) = app.session.doc().ok().and_then(|d| d.node(*id)).cloned() else { return };
            let list = if *fills { &n.fills } else { &n.strokes };
            let Some(mut paint) = list.get(*index).cloned() else { return };
            if let PaintKind::Gradient(g) = &mut paint.kind {
                match which {
                    0 => g.from = u,
                    1 => g.to = u,
                    _ => g.width = u,
                }
            }
            let cmd = if *fills { "fill.set" } else { "stroke.set" };
            let _ = app.session.execute(cmd, &json!({"ids": [id], "index": index, "paint": paint}));
            let _: Option<Paint> = None;
        }
        Drag::Padding { id, side, start, value } => {
            let d = f64::from(match side {
                0 => pos.y - start.y,
                1 => start.x - pos.x,
                2 => start.y - pos.y,
                _ => pos.x - start.x,
            }) / zoom;
            let v = (value + d).round().max(0.0);
            let key = ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"].get(*side).copied().unwrap_or("paddingTop");
            // Shift drags both sides of the axis together.
            let props =
                if shift { if side % 2 == 0 { json!({"paddingVertical": v}) } else { json!({"paddingHorizontal": v}) } } else { json!({ key: v }) };
            let _ = app.session.execute("node.set", &json!({"ids": [id], "props": props}));
        }
        Drag::Gap { id, start, value, vertical } => {
            let d = f64::from(if *vertical { pos.y - start.y } else { pos.x - start.x }) / zoom;
            let v = (value + d).round().max(0.0);
            let _ = app.session.execute("node.set", &json!({"ids": [id], "props": {"gap": v}}));
        }
    }
}
