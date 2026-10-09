//! The floating bottom toolbar (UI3): tool groups with flyouts, comments, actions.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_engine::ToolId;
use serde_json::json;

use crate::FigApp;
use crate::icons::{self, Icon};
use crate::theme::{ACCENT, Tokens, regular};

struct Group {
    key: &'static str,
    tools: &'static [(ToolId, Icon)],
}

const GROUPS: &[Group] = &[
    Group { key: "move", tools: &[(ToolId::Move, Icon::Move), (ToolId::Hand, Icon::Hand), (ToolId::Scale, Icon::Scale)] },
    Group { key: "frame", tools: &[(ToolId::Frame, Icon::Frame), (ToolId::Section, Icon::Section), (ToolId::Slice, Icon::Slice)] },
    Group {
        key: "shape",
        tools: &[
            (ToolId::Rectangle, Icon::Rectangle),
            (ToolId::Line, Icon::Line),
            (ToolId::Arrow, Icon::Arrow),
            (ToolId::Ellipse, Icon::Ellipse),
            (ToolId::Polygon, Icon::Polygon),
            (ToolId::Star, Icon::Star),
        ],
    },
    Group { key: "pen", tools: &[(ToolId::Pen, Icon::Pen), (ToolId::Pencil, Icon::Pencil)] },
    Group { key: "text", tools: &[(ToolId::Text, Icon::Text)] },
    Group { key: "comment", tools: &[(ToolId::Comment, Icon::Comment)] },
];

fn remembered(ui: &Ui, key: &str, default: ToolId) -> ToolId {
    ui.data(|d| d.get_temp::<u8>(egui::Id::new(("tb_last", key)))).and_then(|i| ToolId::ALL.get(i as usize).copied()).unwrap_or(default)
}

pub fn show(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let canvas = app.canvas.rect;
    let cell = 32.0;
    let gap = 4.0;
    let n_cells = GROUPS.len() as f32 + 1.0;
    let width = n_cells * (cell + 10.0) + 16.0 + 64.0;
    let height = 48.0;
    let rect = Rect::from_center_size(pos2(canvas.center().x, canvas.max.y - 12.0 - height / 2.0), vec2(width, height));
    let painter = ui.painter();
    // Shadow + pill.
    for (i, a) in [(6.0, 10u8), (3.0, 16), (1.0, 24)] {
        painter.rect_filled(rect.expand(i).translate(vec2(0.0, i * 0.5)), CornerRadius::same((13.0 + i) as u8), Color32::from_black_alpha(a));
    }
    painter.rect_filled(rect, CornerRadius::same(13), t.panel);
    painter.rect_stroke(rect, CornerRadius::same(13), Stroke::new(1.0, t.border), StrokeKind::Inside);
    let current = app.session.tool.tool;
    let mut x = rect.min.x + 8.0;
    let cy = rect.center().y;
    for g in GROUPS {
        let in_group = g.tools.iter().any(|(tid, _)| *tid == current);
        let shown = if in_group { current } else { remembered(ui, g.key, g.tools.first().map(|x| x.0).unwrap_or(ToolId::Move)) };
        let icon = g.tools.iter().find(|(tid, _)| *tid == shown).map(|(_, i)| *i).unwrap_or(Icon::Move);
        let r = Rect::from_center_size(pos2(x + cell / 2.0, cy), vec2(cell, cell));
        let resp = ui.interact(r, ui.id().with(("tool", g.key)), Sense::click());
        let active = in_group;
        let bg = if active { ACCENT } else if resp.hovered() { t.hover } else { Color32::TRANSPARENT };
        ui.painter().rect_filled(r, CornerRadius::same(5), bg);
        icons::paint(ui.painter(), r.shrink(7.0), icon, if active { Color32::WHITE } else { t.icon });
        let resp = resp.on_hover_text(format!("{}  {}", shown.label(), shown.shortcut()));
        if resp.clicked() {
            app.run("tool.set", json!({"tool": shown.id()}));
        }
        x += cell;
        // Flyout chevron.
        if g.tools.len() > 1 {
            let cr = Rect::from_center_size(pos2(x + 5.0, cy), vec2(10.0, cell));
            let cresp = ui.interact(cr, ui.id().with(("flyout", g.key)), Sense::click());
            if cresp.hovered() {
                ui.painter().rect_filled(cr, CornerRadius::same(4), t.hover);
            }
            icons::paint(ui.painter(), Rect::from_center_size(cr.center(), vec2(10.0, 10.0)), Icon::Chevron, t.icon_secondary);
            if cresp.clicked() {
                app.ui.tool_flyout = if app.ui.tool_flyout == Some(g.key) { None } else { Some(g.key) };
            }
            x += 10.0;
        }
        x += gap;
        if app.ui.tool_flyout == Some(g.key) {
            flyout(app, ui, g, pos2(r.min.x, rect.min.y - 8.0));
        }
    }
    // Separator and Actions.
    ui.painter().line_segment([pos2(x + 2.0, cy - 12.0), pos2(x + 2.0, cy + 12.0)], Stroke::new(1.0, t.border));
    x += 8.0;
    let r = Rect::from_center_size(pos2(x + cell / 2.0, cy), vec2(cell, cell));
    let resp = ui.interact(r, ui.id().with("actions"), Sense::click()).on_hover_text("Actions  ⌘K");
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), r.shrink(7.0), Icon::Actions, t.icon);
    if resp.clicked() {
        app.ui.palette = Some(String::new());
    }
    x += cell + 6.0;
    // Dev mode toggle look-alike: the Code (inspect) panel.
    let r = Rect::from_min_size(pos2(x, cy - 14.0), vec2(56.0, 28.0));
    let resp = ui.interact(r, ui.id().with("devmode"), Sense::click()).on_hover_text("Inspect: code and measurements");
    let on = app.ui.right_tab == crate::RightTab::Inspect;
    ui.painter().rect_filled(r, CornerRadius::same(14), if on { Color32::from_rgb(0x0F, 0xA9, 0x58) } else { t.field });
    let knob = if on { pos2(r.max.x - 14.0, cy) } else { pos2(r.min.x + 14.0, cy) };
    ui.painter().circle_filled(knob, 11.0, Color32::WHITE);
    icons::paint(ui.painter(), Rect::from_center_size(knob, vec2(14.0, 14.0)), Icon::Code, if on { Color32::from_rgb(0x0F, 0xA9, 0x58) } else { t.icon });
    if resp.clicked() {
        app.ui.right_tab = if on { crate::RightTab::Design } else { crate::RightTab::Inspect };
    }
}

fn flyout(app: &mut FigApp, ui: &mut Ui, g: &Group, bottom_left: egui::Pos2) {
    let t = Tokens::get(ui.ctx());
    let item_h = 32.0;
    let w = 220.0;
    let h = item_h * g.tools.len() as f32 + 16.0;
    let mut chosen = None;
    egui::Area::new(egui::Id::new(("flyout_area", g.key))).fixed_pos(pos2(bottom_left.x, bottom_left.y - h)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(13), t.menu);
        let mut y = rect.min.y + 8.0;
        for (tid, icon) in g.tools {
            let r = Rect::from_min_size(pos2(rect.min.x + 8.0, y), vec2(w - 16.0, item_h));
            let resp = ui.interact(r, ui.id().with(("fly", tid.id())), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(5), t.menu_hover);
            }
            if *tid == app.session.tool.tool {
                icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 10.0, r.center().y), vec2(12.0, 12.0)), Icon::Check, t.menu_text);
            }
            icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 32.0, r.center().y), vec2(16.0, 16.0)), *icon, t.menu_text);
            ui.painter().text(pos2(r.min.x + 48.0, r.center().y), Align2::LEFT_CENTER, tid.label(), regular(12.0), t.menu_text);
            ui.painter().text(pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, pretty_shortcut(tid.shortcut()), regular(12.0), t.menu_dim);
            if resp.clicked() {
                chosen = Some(*tid);
            }
            y += item_h;
        }
    });
    if let Some(tid) = chosen {
        if let Some(i) = ToolId::ALL.iter().position(|x| *x == tid) {
            ui.data_mut(|d| d.insert_temp(egui::Id::new(("tb_last", g.key)), i as u8));
        }
        app.run("tool.set", json!({"tool": tid.id()}));
        app.ui.tool_flyout = None;
    }
}

/// "Shift+Cmd+K" → "⇧⌘K" on macOS.
pub fn pretty_shortcut(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mac = cfg!(target_os = "macos");
    if !mac {
        return s.replace("Cmd", "Ctrl").replace("Alt", "Alt");
    }
    let mut out = String::new();
    for part in s.split('+') {
        out.push_str(match part {
            "Shift" => "⇧",
            "Cmd" => "⌘",
            "Alt" => "⌥",
            "Ctrl" => "⌃",
            "Delete" => "⌫",
            "Enter" => "↵",
            "Escape" => "Esc",
            "Tab" => "⇥",
            "PageUp" => "⇞",
            "PageDown" => "⇟",
            other => other,
        });
    }
    out
}
