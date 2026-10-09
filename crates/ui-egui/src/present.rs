//! The prototype player: presents a frame full-window, follows interactions with transitions.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Ui, pos2, vec2};
use figcraft_doc::proto::{Action, Transition, Trigger};
use figcraft_doc::{NodeId, NodeKind};

use crate::FigApp;
use crate::theme::{regular, semibold};

pub struct Presenter {
    pub current: NodeId,
    pub history: Vec<NodeId>,
    /// Transition in progress: (from, kind, started at, duration s).
    pub anim: Option<(NodeId, Transition, f64, f64, figcraft_doc::proto::Easing)>,
    pub overlay: Option<NodeId>,
    texture: Option<(NodeId, egui::TextureHandle, f64)>,
    from_texture: Option<egui::TextureHandle>,
    pub delay_started: f64,
}

impl Presenter {
    pub fn new(start: NodeId) -> Self {
        Self { current: start, history: Vec::new(), anim: None, overlay: None, texture: None, from_texture: None, delay_started: -1.0 }
    }
}

fn frame_texture(app: &FigApp, ctx: &egui::Context, id: NodeId, scale: f64) -> Option<egui::TextureHandle> {
    let doc = app.session.doc().ok()?;
    let pix = figcraft_render::render_node(doc, id, scale)?;
    let img = egui::ColorImage::from_rgba_premultiplied([pix.width() as usize, pix.height() as usize], pix.data());
    Some(ctx.load_texture(format!("present-{}", id.0), img, egui::TextureOptions::LINEAR))
}

fn navigate(p: &mut Presenter, dest: NodeId, tr: Transition, dur: f64, easing: figcraft_doc::proto::Easing, now: f64) {
    if dest == p.current {
        return;
    }
    p.history.push(p.current);
    p.anim = if matches!(tr, Transition::Instant) { None } else { Some((p.current, tr, now, dur.max(0.01), easing)) };
    p.from_texture = p.texture.take().map(|(_, t, _)| t);
    p.current = dest;
    p.delay_started = now;
}

pub fn show(app: &mut FigApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let Some(mut pres) = app.ui.present.take() else { return };
    let bg = app
        .session
        .ds()
        .ok()
        .and_then(|ds| match ds.doc.node(ds.page).map(|n| &n.kind) {
            Some(NodeKind::Page { prototype, .. }) => Some(crate::theme::c32(prototype.background)),
            _ => None,
        })
        .unwrap_or(Color32::from_rgb(0x1E, 0x1E, 0x1E));
    let mut exit = false;
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(bg)).show(ui, |ui| {
        let rect = ui.max_rect();
        let Ok(doc) = app.session.doc().cloned() else { return };
        let Some(frame) = doc.node(pres.current).cloned() else {
            exit = true;
            return;
        };
        // Fit the frame (max 1:1 like Figma's "fit" for small frames).
        let avail = rect.shrink(48.0);
        let scale = (f64::from(avail.width()) / frame.width.max(1.0)).min(f64::from(avail.height()) / frame.height.max(1.0)).min(1.0);
        let ppp = f64::from(ctx.pixels_per_point());
        let need = pres.texture.as_ref().is_none_or(|(id, _, s)| *id != pres.current || (*s - scale * ppp).abs() > 1e-6);
        if need && let Some(tex) = frame_texture(app, &ctx, pres.current, scale * ppp) {
            pres.texture = Some((pres.current, tex, scale * ppp));
        }
        let size = vec2((frame.width * scale) as f32, (frame.height * scale) as f32);
        let target = Rect::from_center_size(rect.center(), size);
        let mut t_anim = 1.0;
        if let Some((_, tr, start, dur, easing)) = pres.anim {
            let raw = ((now - start) / dur).clamp(0.0, 1.0);
            t_anim = easing.eval(raw) as f32;
            if raw >= 1.0 {
                pres.anim = None;
                pres.from_texture = None;
            } else {
                ctx.request_repaint();
            }
            let _ = tr;
        }
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let anim = pres.anim.as_ref().map(|a| a.1);
        match (anim, &pres.from_texture, &pres.texture) {
            (Some(tr), Some(from), Some((_, to, _))) => {
                let w = target.width();
                match tr {
                    Transition::Dissolve | Transition::SmartAnimate => {
                        ui.painter().image(from.id(), target, uv, Color32::WHITE.gamma_multiply(1.0 - t_anim));
                        ui.painter().image(to.id(), target, uv, Color32::WHITE.gamma_multiply(t_anim));
                    }
                    Transition::Push { .. } | Transition::SlideIn { .. } | Transition::MoveIn { .. } => {
                        let dx = w * (1.0 - t_anim);
                        let painter = ui.painter_at(target);
                        if matches!(tr, Transition::Push { .. }) {
                            painter.image(from.id(), target.translate(vec2(dx - w, 0.0)), uv, Color32::WHITE);
                        } else {
                            painter.image(from.id(), target, uv, Color32::WHITE);
                        }
                        painter.image(to.id(), target.translate(vec2(dx, 0.0)), uv, Color32::WHITE);
                    }
                    _ => {
                        ui.painter().image(to.id(), target, uv, Color32::WHITE);
                    }
                }
            }
            (_, _, Some((_, tex, _))) => {
                ui.painter().image(tex.id(), target, uv, Color32::WHITE);
            }
            _ => {}
        }
        // Hotspots: clickable layers with interactions inside the frame.
        let fw = doc.world_transform(pres.current);
        let inv = fw.inverse();
        let mut hot: Vec<(NodeId, Rect)> = Vec::new();
        for id in doc.subtree(pres.current) {
            let Some(n) = doc.node(id) else { continue };
            if n.interactions.is_empty() || !n.visible {
                continue;
            }
            if let Some(b) = doc.world_bounds(id) {
                let a = inv * figcraft_geom::Point::new(b.x0, b.y0);
                let c = inv * figcraft_geom::Point::new(b.x1, b.y1);
                let r = Rect::from_min_max(
                    pos2(target.min.x + (a.x * scale) as f32, target.min.y + (a.y * scale) as f32),
                    pos2(target.min.x + (c.x * scale) as f32, target.min.y + (c.y * scale) as f32),
                );
                hot.push((id, r));
            }
        }
        let click = ui.interact(rect, ui.id().with("present_click"), Sense::click());
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let mut fired = false;
        for (id, r) in hot.iter().rev() {
            let Some(n) = doc.node(*id) else { continue };
            let over = pointer.is_some_and(|p| r.contains(p));
            if over {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            for it in &n.interactions {
                let trigger_hit = match it.trigger {
                    Trigger::OnClick | Trigger::MouseUp | Trigger::MouseDown | Trigger::OnPress => over && click.clicked(),
                    Trigger::OnHover | Trigger::MouseEnter => over,
                    Trigger::AfterDelay { ms } => {
                        *id == pres.current && pres.anim.is_none() && pres.delay_started >= 0.0 && now - pres.delay_started > f64::from(ms) / 1000.0
                    }
                    _ => false,
                };
                if !trigger_hit || fired {
                    continue;
                }
                for a in &it.actions {
                    match a {
                        Action::Navigate { dest } | Action::ChangeTo { dest } => {
                            let top = doc.top_level_frame(*dest).unwrap_or(*dest);
                            navigate(&mut pres, top, it.transition, f64::from(it.duration_ms) / 1000.0, it.easing, now);
                            fired = true;
                        }
                        Action::Back => {
                            if let Some(prev) = pres.history.pop() {
                                pres.from_texture = pres.texture.take().map(|(_, t, _)| t);
                                pres.anim = None;
                                pres.current = prev;
                                fired = true;
                            }
                        }
                        Action::OpenUrl { url } => {
                            app.services.open_url(url);
                            fired = true;
                        }
                        Action::OpenOverlay { dest, .. } | Action::SwapOverlay { dest } => {
                            pres.overlay = Some(*dest);
                            fired = true;
                        }
                        Action::CloseOverlay => {
                            pres.overlay = None;
                            fired = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        if !fired && click.clicked() {
            // Clicking empty space flashes hotspots (Figma behaviour): outline them briefly.
            for (_, r) in &hot {
                ui.painter().rect_stroke(
                    *r,
                    CornerRadius::same(2),
                    egui::Stroke::new(2.0, crate::theme::ACCENT.gamma_multiply(0.6)),
                    egui::StrokeKind::Outside,
                );
            }
        }
        // Overlay frame on top.
        if let Some(ov) = pres.overlay
            && let Some(on) = doc.node(ov)
            && let Some(tex) = frame_texture(app, &ctx, ov, scale * ppp)
        {
            let osize = vec2((on.width * scale) as f32, (on.height * scale) as f32);
            let orect = Rect::from_center_size(target.center(), osize);
            ui.painter().rect_filled(target, CornerRadius::ZERO, Color32::from_black_alpha(80));
            ui.painter().image(tex.id(), orect, uv, Color32::WHITE);
            if click.clicked() && pointer.is_some_and(|p| !orect.contains(p)) {
                pres.overlay = None;
            }
        }
        // Top bar.
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 40.0));
        ui.painter().rect_filled(bar, CornerRadius::ZERO, Color32::from_black_alpha(140));
        ui.painter().text(pos2(bar.min.x + 16.0, bar.center().y), Align2::LEFT_CENTER, &frame.name, semibold(12.0), Color32::WHITE);
        ui.painter().text(
            pos2(bar.max.x - 16.0, bar.center().y),
            Align2::RIGHT_CENTER,
            "Esc to close · ← → for frames",
            regular(11.0),
            Color32::from_gray(200),
        );
        if pres.delay_started < 0.0 {
            pres.delay_started = now;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        // Arrow keys step through top-level frames.
        let frames = doc.top_frames(doc.page_of(pres.current).unwrap_or(pres.current));
        let idx = frames.iter().position(|f| *f == pres.current);
        let (left, right) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight)));
        if let Some(i) = idx {
            if right && let Some(n) = frames.get(i + 1) {
                navigate(&mut pres, *n, Transition::Instant, 0.0, Default::default(), now);
            }
            if left
                && i > 0
                && let Some(n) = frames.get(i - 1)
            {
                navigate(&mut pres, *n, Transition::Instant, 0.0, Default::default(), now);
            }
        }
    });
    if exit || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return;
    }
    app.ui.present = Some(pres);
}
