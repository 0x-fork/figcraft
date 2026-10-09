//! The desktop tab strip: home, one tab per open file, new tab.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::FigApp;
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular};

pub const HEIGHT: f32 = 40.0;

pub fn tab_bar(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("tab_bar").exact_size(HEIGHT).frame(egui::Frame::NONE.fill(t.tab_bar)).show(ui, |ui| {
        let rect = ui.max_rect();
        let mut x = rect.min.x + if cfg!(target_os = "macos") { 76.0 } else { 8.0 };
        // Home.
        let hr = Rect::from_min_size(pos2(x, rect.min.y), vec2(44.0, HEIGHT));
        let hresp = ui.interact(hr, ui.id().with("home"), Sense::click()).on_hover_text("Home");
        let home_active = app.ui.home || app.session.docs.is_empty();
        if home_active {
            ui.painter().rect_filled(hr, CornerRadius::ZERO, t.tab_active);
        } else if hresp.hovered() {
            ui.painter().rect_filled(hr, CornerRadius::ZERO, t.hover);
        }
        icons::paint(ui.painter(), Rect::from_center_size(hr.center(), vec2(16.0, 16.0)), Icon::Home, t.icon);
        if hresp.clicked() {
            app.ui.home = true;
        }
        x += 44.0;
        ui.painter().line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], Stroke::new(1.0, t.border));
        let mut switch = None;
        let mut close = None;
        for (i, d) in app.session.docs.iter().enumerate() {
            let active = i == app.session.active && !app.ui.home;
            let name = d.doc.name.clone();
            let w = (regular(12.0).size * 0.6 * name.chars().count() as f32 + 64.0).clamp(120.0, 220.0);
            let r = Rect::from_min_size(pos2(x, rect.min.y), vec2(w, HEIGHT));
            let resp = ui.interact(r, ui.id().with(("tab", d.uid)), Sense::click());
            if active {
                ui.painter().rect_filled(r, CornerRadius::ZERO, t.tab_active);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::ZERO, t.hover);
            }
            icons::paint(
                ui.painter(),
                Rect::from_center_size(pos2(r.min.x + 18.0, r.center().y), vec2(14.0, 14.0)),
                Icon::Logo,
                crate::theme::ACCENT,
            );
            let label = if d.is_dirty() { format!("{name} •") } else { name };
            let galley = ui.painter().layout(label, regular(12.0), t.text, w - 60.0);
            ui.painter().galley(pos2(r.min.x + 32.0, r.center().y - galley.size().y / 2.0), galley, t.text);
            let cr = Rect::from_center_size(pos2(r.max.x - 16.0, r.center().y), vec2(18.0, 18.0));
            if resp.hovered() || active {
                let cresp = ui.interact(cr, ui.id().with(("tabclose", d.uid)), Sense::click());
                if cresp.hovered() {
                    ui.painter().rect_filled(cr, CornerRadius::same(4), t.pressed);
                }
                icons::paint(ui.painter(), cr.shrink(4.0), Icon::Close, t.icon_secondary);
                if cresp.clicked() {
                    close = Some(i);
                }
            }
            if resp.clicked() && close.is_none() {
                switch = Some(i);
            }
            if resp.middle_clicked() {
                close = Some(i);
            }
            x += w;
            ui.painter().line_segment([pos2(x, rect.min.y + 10.0), pos2(x, rect.max.y - 10.0)], Stroke::new(1.0, t.border));
        }
        // New tab.
        let nr = Rect::from_center_size(pos2(x + 20.0, rect.center().y), vec2(28.0, 28.0));
        let nresp = ui.interact(nr, ui.id().with("newtab"), Sense::click()).on_hover_text("New design file  ⌘N");
        if nresp.hovered() {
            ui.painter().rect_filled(nr, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), nr.shrink(7.0), Icon::Plus, t.icon);
        if nresp.clicked() {
            app.run("file.new", json!({}));
            app.ui.home = false;
        }
        // Right side: theme toggle.
        let tr = Rect::from_center_size(pos2(rect.max.x - 24.0, rect.center().y), vec2(28.0, 28.0));
        let tresp = ui.interact(tr, ui.id().with("theme"), Sense::click()).on_hover_text("Toggle light/dark theme");
        if tresp.hovered() {
            ui.painter().rect_filled(tr, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), tr.shrink(7.0), if app.ui.dark { Icon::Sun } else { Icon::Moon }, t.icon);
        if tresp.clicked() {
            let cmd = if app.ui.dark { "view.themeLight" } else { "view.themeDark" };
            app.run(cmd, json!({}));
        }
        if let Some(i) = switch {
            app.run("doc.switch", json!({"index": i}));
            app.ui.home = false;
        }
        if let Some(i) = close {
            let dirty = app.session.docs.get(i).is_some_and(|d| d.is_dirty());
            if dirty {
                app.ui.find = None;
                app.toast("Unsaved changes: save first, or close again with ⌥ to discard");
                if ui.input(|inp| inp.modifiers.alt) {
                    app.run("file.close", json!({"index": i, "force": true}));
                }
            } else {
                app.run("file.close", json!({"index": i}));
            }
        }
        let _ = Color32::TRANSPARENT;
        let _ = Align2::LEFT_TOP;
    });
}
