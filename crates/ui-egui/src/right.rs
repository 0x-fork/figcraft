//! The right panel: Design, Prototype and Inspect tabs.

use egui::{Align2, Color32, CornerRadius, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_color::{BlendMode, Paint, PaintKind};
use figcraft_doc::*;
use serde_json::{Value, json};

use crate::icons::{self, Icon};
use crate::theme::{ACCENT, Tokens, regular, semibold};
use crate::widgets::{self, FIELD_H, FieldEdit, Lead, PAD};
use crate::{FigApp, RightTab};

/// Common value of `f` across `ids` (None when mixed or empty).
fn common<T: PartialEq + Clone>(doc: &Document, ids: &[NodeId], f: impl Fn(&Node) -> T) -> Option<T> {
    let mut it = ids.iter().filter_map(|i| doc.node(*i));
    let first = f(it.next()?);
    if it.all(|n| f(n) == first) { Some(first) } else { None }
}

impl FigApp {
    /// Set properties on `ids` (one undo step).
    pub fn set_props(&mut self, ids: &[NodeId], props: Value) {
        if ids.is_empty() {
            return;
        }
        self.run("node.set", json!({"ids": ids, "props": props}));
    }

    /// Apply a field edit: scrubs are live previews committed as one step.
    pub fn field(&mut self, ids: &[NodeId], key: &str, edit: Option<FieldEdit>, map: impl Fn(f64) -> Value) {
        match edit {
            Some(FieldEdit::Set(v)) => self.set_props(ids, json!({ key: map(v) })),
            Some(FieldEdit::Scrub(v, done)) => {
                let _ = self.session.begin_interaction("Edit");
                let _ = self.session.execute("node.set", &json!({"ids": ids, "props": { key: map(v) }}));
                if done {
                    let _ = self.session.commit_interaction();
                }
            }
            None => {}
        }
    }
}

pub fn right_panel(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width();
    // Top bar: avatar, present, share.
    let (top, _) = ui.allocate_exact_size(vec2(w, 48.0), Sense::hover());
    ui.painter().circle_filled(pos2(top.min.x + 24.0, top.center().y), 12.0, Color32::from_rgb(0x14, 0xAE, 0x5C));
    ui.painter().text(pos2(top.min.x + 24.0, top.center().y), Align2::CENTER_CENTER, "Y", semibold(11.0), Color32::WHITE);
    let share = Rect::from_min_size(pos2(top.max.x - 72.0, top.center().y - 12.0), vec2(60.0, 24.0));
    let sresp = ui.interact(share, ui.id().with("share"), Sense::click()).on_hover_text("Export and share");
    ui.painter().rect_filled(share, CornerRadius::same(5), if sresp.hovered() { t.accent_hover } else { ACCENT });
    ui.painter().text(share.center(), Align2::CENTER_CENTER, "Share", semibold(11.0), Color32::WHITE);
    if sresp.clicked() {
        app.ui.export_preview = true;
    }
    let play = Rect::from_min_size(pos2(share.min.x - 44.0, top.center().y - 12.0), vec2(36.0, 24.0));
    let presp = ui.interact(play, ui.id().with("play"), Sense::click()).on_hover_text("Present  ⌥⌘↵");
    if presp.hovered() {
        ui.painter().rect_filled(play, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(play.center() - vec2(5.0, 0.0), vec2(12.0, 12.0)), Icon::Play, t.icon);
    icons::paint(ui.painter(), Rect::from_center_size(play.center() + vec2(9.0, 0.0), vec2(10.0, 10.0)), Icon::Chevron, t.icon_secondary);
    if presp.clicked() {
        app.run("proto.present", json!({}));
    }
    // Tabs row.
    let (tabs, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
    ui.painter().line_segment([tabs.left_top(), tabs.right_top()], Stroke::new(1.0, t.border));
    let mut x = tabs.min.x + 8.0;
    for (tab, label) in [(RightTab::Design, "Design"), (RightTab::Prototype, "Prototype")] {
        let galley = ui.painter().layout_no_wrap(label.into(), semibold(11.0), t.text);
        let r = Rect::from_min_size(pos2(x, tabs.center().y - 12.0), vec2(galley.size().x + 16.0, 24.0));
        let resp = ui.interact(r, ui.id().with(("rtab", label)), Sense::click());
        let active = app.ui.right_tab == tab;
        if active {
            ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
        }
        ui.painter().galley(pos2(r.min.x + 8.0, r.center().y - galley.size().y / 2.0), galley, if active { t.text } else { t.text_secondary });
        if resp.clicked() {
            app.ui.right_tab = tab;
        }
        x = r.max.x + 2.0;
    }
    // Zoom menu.
    let zoom = app.session.ds().map(|d| d.view.zoom).unwrap_or(1.0);
    let zr = Rect::from_min_size(pos2(tabs.max.x - 64.0, tabs.center().y - 12.0), vec2(56.0, 24.0));
    let zresp = ui.interact(zr, ui.id().with("zoommenu"), Sense::click());
    if zresp.hovered() {
        ui.painter().rect_filled(zr, CornerRadius::same(5), t.hover);
    }
    ui.painter().text(pos2(zr.min.x + 6.0, zr.center().y), Align2::LEFT_CENTER, format!("{}%", (zoom * 100.0).round()), regular(11.0), t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(zr.max.x - 9.0, zr.center().y), vec2(10.0, 10.0)), Icon::Chevron, t.icon_secondary);
    egui::Popup::menu(&zresp).width(200.0).show(|ui| {
        for (label, cmd) in [
            ("Zoom in", "view.zoomIn"),
            ("Zoom out", "view.zoomOut"),
            ("Zoom to fit", "view.zoomFit"),
            ("Zoom to 50%", "view.zoom50"),
            ("Zoom to 100%", "view.zoom100"),
            ("Zoom to 200%", "view.zoom200"),
            ("Zoom to selection", "view.zoomSelection"),
        ] {
            if menu_item(ui, label, figcraft_engine::catalog::shortcut(cmd).unwrap_or("")) {
                app.run(cmd, json!({}));
                ui.close();
            }
        }
        ui.separator();
        for (label, cmd) in [("Pixel preview", "view.pixelPreview"), ("Pixel grid", "view.pixelGrid"), ("Snap to pixel grid", "view.snapPixel"), ("Layout grids", "view.layoutGrids"), ("Rulers", "view.rulers"), ("Outlines", "view.outlines")] {
            if menu_item(ui, label, figcraft_engine::catalog::shortcut(cmd).unwrap_or("")) {
                app.run(cmd, json!({}));
                ui.close();
            }
        }
    });
    ui.painter().line_segment([tabs.left_bottom(), tabs.right_bottom()], Stroke::new(1.0, t.border));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
        match app.ui.right_tab {
            RightTab::Design => design(app, ui),
            RightTab::Prototype => prototype(app, ui),
            RightTab::Inspect => inspect(app, ui),
        }
        ui.add_space(80.0);
    });
}

pub fn menu_item(ui: &mut Ui, label: &str, shortcut: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width().max(180.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), ACCENT);
    }
    let col = if resp.hovered() { Color32::WHITE } else { t.text };
    ui.painter().text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, label, regular(11.5), col);
    if !shortcut.is_empty() {
        ui.painter().text(pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, crate::toolbar::pretty_shortcut(shortcut), regular(11.0), if resp.hovered() { Color32::WHITE } else { t.text_secondary });
    }
    resp.clicked()
}

fn row(ui: &mut Ui, h: f32, add: impl FnOnce(&mut Ui)) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(PAD, (h - FIELD_H) / 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().item_spacing = vec2(8.0, 0.0);
    add(&mut child);
}

fn design(app: &mut FigApp, ui: &mut Ui) {
    let Ok(ds) = app.session.ds() else { return };
    let doc = ds.doc.clone();
    let sel = ds.selection.clone();
    if sel.is_empty() {
        page_section(app, ui, &doc);
        return;
    }
    header(app, ui, &doc, &sel);
    widgets::separator(ui);
    position(app, ui, &doc, &sel);
    widgets::separator(ui);
    layout(app, ui, &doc, &sel);
    widgets::separator(ui);
    appearance(app, ui, &doc, &sel);
    widgets::separator(ui);
    if sel.iter().any(|i| matches!(doc.node(*i).map(|n| &n.kind), Some(NodeKind::Text(_)))) {
        typography(app, ui, &doc, &sel);
        widgets::separator(ui);
    }
    paints(app, ui, &doc, &sel, true);
    widgets::separator(ui);
    paints(app, ui, &doc, &sel, false);
    widgets::separator(ui);
    effects(app, ui, &doc, &sel);
    widgets::separator(ui);
    if sel.iter().all(|i| doc.node(*i).is_some_and(|n| n.kind.is_frame_like())) {
        grids(app, ui, &doc, &sel);
        widgets::separator(ui);
    }
    export(app, ui, &doc, &sel);
}

fn page_section(app: &mut FigApp, ui: &mut Ui, doc: &Document) {
    let t = Tokens::get(ui.ctx());
    let Ok(page) = app.session.page() else { return };
    let bg = match doc.node(page).map(|n| &n.kind) {
        Some(NodeKind::Page { background, .. }) => *background,
        _ => figcraft_color::Color::DEFAULT_PAGE,
    };
    widgets::section_header(ui, "Page", &[], false);
    let mut open_picker = false;
    let mut new_hex = None;
    row(ui, 32.0, |ui| {
        let p = Paint::solid(bg);
        if widgets::swatch(ui, &p, 16.0).clicked() {
            open_picker = true;
        }
        if let Some(h) = widgets::text_field(ui, Id::new("pagebg"), &bg.to_hex(), 96.0, "") {
            new_hex = Some(h);
        }
        widgets::label(ui, "100 %", t.text);
    });
    if open_picker {
        app.ui.picker = Some(crate::colorpicker::PickerTarget::PageBackground);
    }
    if let Some(h) = new_hex
        && let Some(c) = figcraft_color::Color::from_hex(&h)
    {
        app.run("page.setBackground", json!({"color": c.to_css_hex()}));
    }
    widgets::separator(ui);
    // Local variables & styles.
    let vars = doc.variables.variables.len();
    widgets::section_header(ui, "Local variables", &[], vars == 0);
    row(ui, 32.0, |ui| {
        let r = widgets::dropdown(ui, &format!("Open variables ({vars})"), 200.0, Some(Icon::Styles));
        if r.clicked() {
            app.ui.preferences = false;
            app.ui.palette = None;
            ui.data_mut(|d| d.insert_temp(Id::new("variables_open"), true));
        }
    });
    widgets::separator(ui);
    let styles: Vec<(String, String, String)> = doc.styles.iter().map(|s| (s.id.clone(), s.name.clone(), s.kind.type_name().to_string())).collect();
    widgets::section_header(ui, "Local styles", &[], styles.is_empty());
    for (_, name, ty) in &styles {
        row(ui, 28.0, |ui| {
            let icon = match ty.as_str() {
                "text" => Icon::Text,
                "effect" => Icon::Effects,
                "grid" => Icon::Grid,
                _ => Icon::Fill,
            };
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), r, icon, t.icon_secondary);
            widgets::label(ui, name, t.text);
        });
    }
    widgets::separator(ui);
    let all: Vec<NodeId> = doc.node(page).map(|p| p.children.clone()).unwrap_or_default();
    widgets::section_header(ui, "Export", &[(Icon::Plus, "Export page")], false);
    row(ui, 32.0, |ui| {
        let w = ui.available_width();
        let (r, resp) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::click());
        ui.painter().rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, t.border), StrokeKind::Inside);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "Export page as PNG", regular(11.0), t.text);
        if resp.clicked() {
            export_now(app, &all, "png", 1.0);
        }
    });
}

fn header(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let t = Tokens::get(ui.ctx());
    let label = match common(doc, sel, |n| n.kind.label()) {
        Some(l) if sel.len() == 1 => l.to_string(),
        Some(l) => format!("{l}s"),
        None => "Mixed".into(),
    };
    let is_frame = sel.len() == 1 && sel.first().and_then(|i| doc.node(*i)).is_some_and(|n| matches!(n.kind, NodeKind::Frame));
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
    let lr = Rect::from_min_size(pos2(r.min.x + 8.0, r.center().y - 12.0), vec2(120.0, 24.0));
    let resp = ui.interact(lr, ui.id().with("typedd"), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(lr, CornerRadius::same(5), t.hover);
    }
    ui.painter().text(pos2(lr.min.x + 8.0, lr.center().y), Align2::LEFT_CENTER, &label, semibold(11.0), t.text);
    if is_frame {
        icons::paint(ui.painter(), Rect::from_center_size(pos2(lr.min.x + 18.0 + label.len() as f32 * 6.5, lr.center().y), vec2(10.0, 10.0)), Icon::Chevron, t.icon_secondary);
        let ids: Vec<NodeId> = sel.to_vec();
        egui::Popup::menu(&resp).width(260.0).show(|ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                let mut cat = "";
                for (c, name, pw, ph) in figcraft_engine::cmd::create::FRAME_PRESETS {
                    if *c != cat {
                        cat = c;
                        ui.label(egui::RichText::new(*c).font(semibold(11.0)).color(t.text_secondary));
                    }
                    if menu_item(ui, name, &format!("{}×{}", pw, ph)) {
                        app.set_props(&ids, json!({"width": pw, "height": ph}));
                        ui.close();
                    }
                }
            });
        });
    }
    // Quick actions on the right.
    let mut x = r.max.x - 8.0;
    let actions: Vec<(Icon, &str, &str)> = vec![(Icon::Component, "Create component  ⌥⌘K", "component.create"), (Icon::Mask, "Use as mask", "object.mask"), (Icon::Union, "Boolean groups", "bool.union")];
    for (icon, tip, cmd) in actions.into_iter().rev() {
        let br = Rect::from_min_size(pos2(x - 24.0, r.center().y - 12.0), vec2(24.0, 24.0));
        let bresp = ui.interact(br, ui.id().with(("qa", cmd)), Sense::click()).on_hover_text(tip);
        if bresp.hovered() {
            ui.painter().rect_filled(br, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), br.shrink(5.0), icon, t.icon);
        if cmd == "bool.union" {
            egui::Popup::menu(&bresp).width(180.0).show(|ui| {
                for (l, c) in [("Union", "bool.union"), ("Subtract", "bool.subtract"), ("Intersect", "bool.intersect"), ("Exclude", "bool.exclude"), ("Flatten", "vector.flatten")] {
                    if menu_item(ui, l, figcraft_engine::catalog::shortcut(c).unwrap_or("")) {
                        app.run(c, json!({}));
                        ui.close();
                    }
                }
            });
        } else if bresp.clicked() {
            app.run(cmd, json!({}));
        }
        x -= 26.0;
    }
    // Instance row.
    if sel.len() == 1
        && let Some(n) = sel.first().and_then(|i| doc.node(*i))
        && let NodeKind::Instance { component, props } = &n.kind
    {
        let comp_name = doc.node(*component).map(|c| c.name.clone()).unwrap_or_else(|| "Missing component".into());
        let set = doc.node(*component).and_then(|c| c.parent).filter(|p| doc.node(*p).is_some_and(|pn| matches!(pn.kind, NodeKind::ComponentSet { .. })));
        row(ui, 32.0, |ui| {
            let (ir, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), ir, Icon::Instance, crate::theme::COMPONENT);
            widgets::label(ui, &comp_name, t.text);
        });
        // Variant properties.
        if let Some(set) = set {
            let mut opts: std::collections::BTreeMap<String, Vec<String>> = Default::default();
            for c in doc.node(set).map(|s| s.children.clone()).unwrap_or_default() {
                if let Some(cn) = doc.node(c) {
                    for (k, v) in figcraft_engine::components::variant_props(&cn.name) {
                        let e = opts.entry(k).or_default();
                        if !e.contains(&v) {
                            e.push(v);
                        }
                    }
                }
            }
            let current = doc.node(*component).map(|c| figcraft_engine::components::variant_props(&c.name)).unwrap_or_default();
            for (k, vals) in opts {
                let cur = current.get(&k).cloned().unwrap_or_default();
                let id = n.id;
                row(ui, 32.0, |ui| {
                    widgets::label(ui, &k, t.text_secondary);
                    let resp = widgets::dropdown(ui, &cur, 120.0, None);
                    egui::Popup::menu(&resp).show(|ui| {
                        for v in &vals {
                            if menu_item(ui, v, "") {
                                app.run("instance.setProperties", json!({"ids": [id], "properties": {k.clone(): v}}));
                                ui.close();
                            }
                        }
                    });
                });
            }
        }
        for (k, v) in props.clone() {
            let id = n.id;
            row(ui, 32.0, |ui| {
                widgets::label(ui, &k, t.text_secondary);
                match v {
                    Value::Bool(b) => {
                        if widgets::checkbox(ui, b, "") {
                            app.run("instance.setProperties", json!({"ids": [id], "properties": {k.clone(): !b}}));
                        }
                    }
                    Value::String(s) => {
                        if let Some(ns) = widgets::text_field(ui, Id::new(("iprop", id, k.as_str())), &s, 120.0, "") {
                            app.run("instance.setProperties", json!({"ids": [id], "properties": {k.clone(): ns}}));
                        }
                    }
                    _ => {}
                }
            });
        }
        let id = n.id;
        row(ui, 32.0, |ui| {
            if icons::button(ui, Icon::Reset, 24.0, false, "Reset all changes").clicked() {
                app.run("component.resetInstance", json!({"ids": [id]}));
            }
            if icons::button(ui, Icon::Detach, 24.0, false, "Detach instance  ⌥⌘B").clicked() {
                app.run("component.detach", json!({"ids": [id]}));
            }
            if icons::button(ui, Icon::Component, 24.0, false, "Go to main component").clicked() {
                app.run("component.goToMain", json!({"id": id}));
            }
            if icons::button(ui, Icon::Export, 24.0, false, "Push changes to main component").clicked() {
                app.run("component.pushChanges", json!({"id": id}));
            }
        });
    }
}

fn position(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    widgets::section_header(ui, "Position", &[], false);
    let ids = sel.to_vec();
    row(ui, 32.0, |ui| {
        let items = [
            (Icon::AlignLeft, "Align left  ⌥A"),
            (Icon::AlignHCenter, "Align horizontal centers  ⌥H"),
            (Icon::AlignRight, "Align right  ⌥D"),
            (Icon::AlignTop, "Align top  ⌥W"),
            (Icon::AlignVCenter, "Align vertical centers  ⌥V"),
            (Icon::AlignBottom, "Align bottom  ⌥S"),
        ];
        let cmds = ["align.left", "align.hcenter", "align.right", "align.top", "align.vcenter", "align.bottom"];
        let w = ((ui.available_width() - 8.0) / 2.0 / 3.0).max(20.0);
        if let Some(i) = widgets::segmented(ui, &items[..3], None, w) {
            app.run(cmds[i], json!({"ids": ids}));
        }
        if let Some(i) = widgets::segmented(ui, &items[3..], None, w) {
            app.run(cmds[i + 3], json!({"ids": ids}));
        }
    });
    let x = common(doc, sel, |n| (n.transform.as_coeffs()[4] * 100.0).round() / 100.0);
    let y = common(doc, sel, |n| (n.transform.as_coeffs()[5] * 100.0).round() / 100.0);
    let rot = common(doc, sel, |n| (figcraft_geom::rotation_degrees(n.transform) * 100.0).round() / 100.0);
    let half = (ui.available_width() - PAD * 2.0 - 8.0) / 2.0;
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(ui, Id::new("pos_x"), Lead::Text("X"), x, "", half, 1.0);
        app.field(&ids, "x", e, |v| json!(v));
        let e = widgets::number_field(ui, Id::new("pos_y"), Lead::Text("Y"), y, "", half, 1.0);
        app.field(&ids, "y", e, |v| json!(v));
    });
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(ui, Id::new("pos_rot"), Lead::Icon(Icon::Rotate), rot, "°", half, 1.0);
        app.field(&ids, "rotation", e, |v| json!(v));
        if icons::button(ui, Icon::Rotate, 24.0, false, "Rotate 90° right").clicked() {
            app.run("object.rotateRight", json!({"ids": ids}));
        }
        if icons::button(ui, Icon::FlipH, 24.0, false, "Flip horizontal  ⇧H").clicked() {
            app.run("object.flipH", json!({"ids": ids}));
        }
        if icons::button(ui, Icon::FlipV, 24.0, false, "Flip vertical  ⇧V").clicked() {
            app.run("object.flipV", json!({"ids": ids}));
        }
    });
    // Constraints (children of non-auto-layout frames).
    let in_frame = sel.iter().all(|i| doc.node(*i).and_then(|n| n.parent).and_then(|p| doc.node(p)).is_some_and(|p| p.kind.is_frame_like() && p.layout.is_none()));
    if in_frame {
        let c = common(doc, sel, |n| n.constraints);
        row(ui, 32.0, |ui| {
            let t = Tokens::get(ui.ctx());
            widgets::label(ui, "Constraints", t.text_secondary);
        });
        row(ui, 32.0, |ui| {
            let h = c.map(|c| format!("{:?}", c.horizontal)).unwrap_or_else(|| "Mixed".into());
            let resp = widgets::dropdown(ui, &pretty(&h), half, None);
            egui::Popup::menu(&resp).show(|ui| {
                for (l, v) in [("Left", "left"), ("Right", "right"), ("Left & right", "leftRight"), ("Center", "center"), ("Scale", "scale")] {
                    if menu_item(ui, l, "") {
                        app.set_props(&ids, json!({"constraints": {"horizontal": v}}));
                        ui.close();
                    }
                }
            });
            let v = c.map(|c| format!("{:?}", c.vertical)).unwrap_or_else(|| "Mixed".into());
            let resp = widgets::dropdown(ui, &pretty(&v), half, None);
            egui::Popup::menu(&resp).show(|ui| {
                for (l, val) in [("Top", "top"), ("Bottom", "bottom"), ("Top & bottom", "topBottom"), ("Center", "center"), ("Scale", "scale")] {
                    if menu_item(ui, l, "") {
                        app.set_props(&ids, json!({"constraints": {"vertical": val}}));
                        ui.close();
                    }
                }
            });
        });
    }
}

fn pretty(s: &str) -> String {
    match s {
        "LeftRight" => "Left & right".into(),
        "TopBottom" => "Top & bottom".into(),
        other => other.to_string(),
    }
}

fn layout(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let t = Tokens::get(ui.ctx());
    let ids = sel.to_vec();
    let frames = sel.iter().all(|i| doc.node(*i).is_some_and(|n| n.kind.is_frame_like()));
    let al = common(doc, sel, |n| n.layout.clone());
    let has_al = matches!(al, Some(Some(_)));
    let title = if has_al { "Auto layout" } else { "Layout" };
    let btns: Vec<(Icon, &str)> = if frames && !has_al { vec![(Icon::Plus, "Add auto layout  ⇧A")] } else if has_al { vec![(Icon::Minus, "Remove auto layout  ⌥⇧A")] } else { vec![] };
    if let Some(_b) = widgets::section_header(ui, title, &btns, false) {
        app.run(if has_al { "layout.remove" } else { "layout.add" }, json!({"ids": ids}));
    }
    let half = (ui.available_width() - PAD * 2.0 - 8.0) / 2.0;
    if let Some(Some(l)) = &al {
        // Direction.
        row(ui, 32.0, |ui| {
            let items = [(Icon::LayoutV, "Vertical"), (Icon::LayoutH, "Horizontal"), (Icon::LayoutWrap, "Wrap"), (Icon::LayoutGrid, "Grid")];
            let active = match (l.mode, l.wrap) {
                (LayoutMode::Vertical, _) => 0,
                (LayoutMode::Horizontal, false) => 1,
                (LayoutMode::Horizontal, true) => 2,
                (LayoutMode::Grid, _) => 3,
            };
            if let Some(i) = widgets::segmented(ui, &items, Some(active), 32.0) {
                let p = match i {
                    0 => json!({"layout": "vertical", "wrap": false}),
                    1 => json!({"layout": "horizontal", "wrap": false}),
                    2 => json!({"layout": "horizontal", "wrap": true}),
                    _ => json!({"layout": "grid"}),
                };
                app.set_props(&ids, p);
            }
        });
    }
    // W / H with sizing.
    let w = common(doc, sel, |n| (n.width * 100.0).round() / 100.0);
    let h = common(doc, sel, |n| (n.height * 100.0).round() / 100.0);
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(ui, Id::new("lay_w"), Lead::Text("W"), w, "", half, 1.0);
        app.field(&ids, "width", e, |v| json!(v.max(0.0)));
        let e = widgets::number_field(ui, Id::new("lay_h"), Lead::Text("H"), h, "", half, 1.0);
        app.field(&ids, "height", e, |v| json!(v.max(0.0)));
    });
    let parent_al = sel.iter().all(|i| doc.node(*i).and_then(|n| n.parent).and_then(|p| doc.node(p)).is_some_and(|p| p.layout.is_some()));
    let is_text = sel.iter().all(|i| matches!(doc.node(*i).map(|n| &n.kind), Some(NodeKind::Text(_))));
    if has_al || parent_al || is_text {
        let sh = common(doc, sel, |n| n.child_layout.sizing_h);
        let sv = common(doc, sel, |n| n.child_layout.sizing_v);
        row(ui, 32.0, |ui| {
            for (axis, cur, key) in [("W", sh, "sizingH"), ("H", sv, "sizingV")] {
                let label = cur.map(|s| s.label()).unwrap_or("Mixed");
                let resp = widgets::dropdown(ui, &format!("{label} {}", if axis == "W" { "width" } else { "height" }), half, Some(match cur {
                    Some(Sizing::Hug) => Icon::Hug,
                    Some(Sizing::Fill) => Icon::FillContainer,
                    _ => Icon::Fixed,
                }));
                let ids2 = ids.clone();
                egui::Popup::menu(&resp).show(|ui| {
                    let mut opts = vec![("Fixed", "fixed")];
                    if has_al || is_text {
                        opts.push(("Hug contents", "hug"));
                    }
                    if parent_al {
                        opts.push(("Fill container", "fill"));
                    }
                    for (l, v) in opts {
                        if menu_item(ui, l, "") {
                            app.set_props(&ids2, json!({key: v}));
                            ui.close();
                        }
                    }
                });
            }
        });
    }
    if let Some(Some(l)) = &al {
        // Alignment grid + gap.
        row(ui, 72.0, |ui| {
            let (grid, _) = ui.allocate_exact_size(vec2(72.0, 64.0), Sense::hover());
            ui.painter().rect_filled(grid, CornerRadius::same(5), t.field);
            let horizontal = l.mode != LayoutMode::Vertical;
            for yi in 0..3 {
                for xi in 0..3 {
                    let c = pos2(grid.min.x + 12.0 + xi as f32 * 24.0, grid.min.y + 10.0 + yi as f32 * 22.0);
                    let r = Rect::from_center_size(c, vec2(22.0, 20.0));
                    let resp = ui.interact(r, Id::new(("al_grid", xi, yi)), Sense::click());
                    let (main_i, cross_i) = if horizontal { (xi, yi) } else { (yi, xi) };
                    let cur_main = match l.main_align {
                        MainAlign::Min => 0,
                        MainAlign::Center => 1,
                        MainAlign::Max => 2,
                        MainAlign::SpaceBetween => 9,
                    };
                    let cur_cross = match l.cross_align {
                        CrossAlign::Min | CrossAlign::Baseline => 0,
                        CrossAlign::Center => 1,
                        CrossAlign::Max => 2,
                    };
                    let on = (cur_main == main_i || cur_main == 9) && cur_cross == cross_i;
                    if on {
                        ui.painter().rect_filled(Rect::from_center_size(c, vec2(if horizontal { 4.0 } else { 12.0 }, if horizontal { 12.0 } else { 4.0 })), CornerRadius::same(1), ACCENT);
                    } else {
                        ui.painter().circle_filled(c, if resp.hovered() { 2.5 } else { 1.5 }, if resp.hovered() { ACCENT } else { t.text_tertiary });
                    }
                    if resp.clicked() {
                        let m = ["min", "center", "max"][main_i];
                        let cr = ["min", "center", "max"][cross_i];
                        let main = if l.main_align == MainAlign::SpaceBetween { "spaceBetween" } else { m };
                        app.set_props(&ids, json!({"mainAlign": main, "crossAlign": cr}));
                    }
                }
            }
            ui.vertical(|ui| {
                let auto = l.main_align == MainAlign::SpaceBetween;
                let e = widgets::number_field(ui, Id::new("al_gap"), Lead::Icon(Icon::Gap), if auto { None } else { Some(l.gap) }, "", 96.0, 1.0);
                app.field(&ids, "gap", e, |v| json!(v));
                ui.add_space(8.0);
                let resp = widgets::dropdown(ui, if auto { "Auto gap" } else { "Fixed gap" }, 96.0, None);
                egui::Popup::menu(&resp).show(|ui| {
                    if menu_item(ui, "Fixed", "") {
                        app.set_props(&ids, json!({"mainAlign": "min"}));
                        ui.close();
                    }
                    if menu_item(ui, "Auto (space between)", "") {
                        app.set_props(&ids, json!({"mainAlign": "spaceBetween"}));
                        ui.close();
                    }
                });
            });
        });
        // Padding.
        let [pt, pr, pb, pl] = l.padding;
        row(ui, 32.0, |ui| {
            let e = widgets::number_field(ui, Id::new("al_padh"), Lead::Icon(Icon::PaddingH), if pl == pr { Some(pl) } else { None }, "", half, 1.0);
            app.field(&ids, "paddingHorizontal", e, |v| json!(v.max(0.0)));
            let e = widgets::number_field(ui, Id::new("al_padv"), Lead::Icon(Icon::PaddingV), if pt == pb { Some(pt) } else { None }, "", half, 1.0);
            app.field(&ids, "paddingVertical", e, |v| json!(v.max(0.0)));
        });
        if l.mode == LayoutMode::Grid {
            row(ui, 32.0, |ui| {
                let e = widgets::number_field(ui, Id::new("al_cols"), Lead::Text("Cols"), Some(f64::from(l.grid_columns)), "", half, 1.0);
                app.field(&ids, "gridColumns", e, |v| json!(v.max(1.0)));
                let e = widgets::number_field(ui, Id::new("al_rows"), Lead::Text("Rows"), Some(f64::from(l.grid_rows)), "", half, 1.0);
                app.field(&ids, "gridRows", e, |v| json!(v.max(1.0)));
            });
        }
    }
    if frames {
        let clip = common(doc, sel, |n| n.clip);
        row(ui, 32.0, |ui| {
            if widgets::checkbox(ui, clip.unwrap_or(false), "Clip content") {
                app.set_props(&ids, json!({"clip": !clip.unwrap_or(false)}));
            }
        });
    }
    if parent_al {
        let abs = common(doc, sel, |n| n.child_layout.absolute);
        row(ui, 32.0, |ui| {
            if widgets::checkbox(ui, abs.unwrap_or(false), "Ignore auto layout (absolute position)") {
                app.set_props(&ids, json!({"absolute": !abs.unwrap_or(false)}));
            }
        });
    }
}

fn appearance(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let ids = sel.to_vec();
    let visible = common(doc, sel, |n| n.visible).unwrap_or(true);
    if let Some(i) = widgets::section_header(ui, "Appearance", &[(Icon::Blend, "Blend mode"), (if visible { Icon::Eye } else { Icon::EyeOff }, "Show/Hide  ⇧⌘H")], false)
        && i == 1
    {
        app.run("object.toggleVisible", json!({"ids": ids}));
    }
    let half = (ui.available_width() - PAD * 2.0 - 8.0) / 2.0;
    let op = common(doc, sel, |n| (f64::from(n.opacity) * 100.0).round());
    let radius_ok = sel.iter().all(|i| doc.node(*i).is_some_and(|n| n.kind.has_corner_radius()));
    let r = common(doc, sel, |n| n.corners.uniform().map(|r| (r * 100.0).round() / 100.0));
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(ui, Id::new("ap_op"), Lead::Icon(Icon::Opacity), op, "%", half, 1.0);
        app.field(&ids, "opacity", e, |v| json!((v / 100.0).clamp(0.0, 1.0)));
        if radius_ok {
            let e = widgets::number_field(ui, Id::new("ap_rad"), Lead::Icon(Icon::CornerRadius), r.flatten(), "", half - 32.0, 1.0);
            app.field(&ids, "cornerRadius", e, |v| json!(v.max(0.0)));
            let open = ui.data(|d| d.get_temp::<bool>(Id::new("indep_corners"))).unwrap_or(false);
            if icons::button(ui, Icon::IndependentCorners, 24.0, open, "Independent corners").clicked() {
                ui.data_mut(|d| d.insert_temp(Id::new("indep_corners"), !open));
            }
        }
    });
    if radius_ok && ui.data(|d| d.get_temp::<bool>(Id::new("indep_corners"))).unwrap_or(false) {
        let radii = common(doc, sel, |n| n.corners.radii);
        let keys = ["topLeftRadius", "topRightRadius", "bottomRightRadius", "bottomLeftRadius"];
        row(ui, 32.0, |ui| {
            for (k, key) in keys.iter().enumerate().take(2) {
                let e = widgets::number_field(ui, Id::new(("corner", k)), Lead::Icon(Icon::CornerRadius), radii.map(|r| r[k]), "", half, 1.0);
                app.field(&ids, key, e, |v| json!(v.max(0.0)));
            }
        });
        row(ui, 32.0, |ui| {
            for (k, key) in keys.iter().enumerate().skip(2) {
                let e = widgets::number_field(ui, Id::new(("corner", k)), Lead::Icon(Icon::CornerRadius), radii.map(|r| r[k]), "", half, 1.0);
                app.field(&ids, key, e, |v| json!(v.max(0.0)));
            }
        });
        let sm = common(doc, sel, |n| (n.corners.smoothing * 100.0).round());
        row(ui, 32.0, |ui| {
            let e = widgets::number_field(ui, Id::new("smoothing"), Lead::Text("Smoothing"), sm, "%", half * 2.0 + 8.0, 1.0);
            app.field(&ids, "cornerSmoothing", e, |v| json!((v / 100.0).clamp(0.0, 1.0)));
        });
    }
    let blend = common(doc, sel, |n| n.blend);
    row(ui, 32.0, |ui| {
        let label = blend.map(|b| b.label()).unwrap_or("Mixed");
        let resp = widgets::dropdown(ui, label, half * 2.0 + 8.0, Some(Icon::Blend));
        egui::Popup::menu(&resp).width(200.0).show(|ui| {
            for b in BlendMode::ALL {
                if menu_item(ui, b.label(), "") {
                    app.set_props(&ids, json!({"blend": b.css()}));
                    ui.close();
                }
                if matches!(b, BlendMode::Normal | BlendMode::ColorBurn | BlendMode::ColorDodge | BlendMode::HardLight | BlendMode::Exclusion) {
                    ui.separator();
                }
            }
        });
    });
}

fn paints(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId], fills: bool) {
    let t = Tokens::get(ui.ctx());
    let ids = sel.to_vec();
    let list = common(doc, sel, |n| if fills { n.fills.clone() } else { n.strokes.clone() });
    let title = if fills { "Fill" } else { "Stroke" };
    let empty = list.as_ref().is_none_or(|l| l.is_empty());
    let style_name = common(doc, sel, |n| if fills { n.styles.fill.clone() } else { n.styles.stroke.clone() }).flatten().and_then(|s| doc.style(&s).map(|st| st.name.clone()));
    if let Some(i) = widgets::section_header(ui, title, &[(Icon::Styles, "Styles"), (Icon::Plus, if fills { "Add fill" } else { "Add stroke" })], empty) {
        if i == 1 {
            app.run(if fills { "fill.add" } else { "stroke.add" }, json!({"ids": ids}));
        } else {
            ui.data_mut(|d| d.insert_temp(Id::new(("style_menu", fills)), true));
        }
    }
    style_picker(app, ui, doc, &ids, fills);
    if let Some(name) = style_name {
        row(ui, 32.0, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Styles, t.icon_secondary);
            widgets::label(ui, &name, t.text);
            if icons::button(ui, Icon::Detach, 24.0, false, "Detach style").clicked() {
                app.run("style.detach", json!({"ids": ids, "which": if fills { "fill" } else { "stroke" }}));
            }
        });
    }
    match &list {
        None => row(ui, 32.0, |ui| widgets::label(ui, "Click + to replace mixed content", t.text_secondary)),
        Some(l) => {
            // Top of the list paints on top: show in reverse like Figma.
            for (i, p) in l.iter().enumerate().rev() {
                paint_row(app, ui, &ids, i, p, fills);
            }
        }
    }
    if !fills && !empty {
        let weight = common(doc, sel, |n| n.stroke.weight);
        let align = common(doc, sel, |n| n.stroke.align);
        let half = (ui.available_width() - PAD * 2.0 - 8.0) / 2.0;
        row(ui, 32.0, |ui| {
            let resp = widgets::dropdown(ui, match align {
                Some(StrokeAlign::Inside) => "Inside",
                Some(StrokeAlign::Center) => "Center",
                Some(StrokeAlign::Outside) => "Outside",
                None => "Mixed",
            }, half, None);
            egui::Popup::menu(&resp).show(|ui| {
                for (l, v) in [("Inside", "inside"), ("Center", "center"), ("Outside", "outside")] {
                    if menu_item(ui, l, "") {
                        app.set_props(&ids, json!({"strokeAlign": v}));
                        ui.close();
                    }
                }
            });
            let e = widgets::number_field(ui, Id::new("stroke_w"), Lead::Icon(Icon::Menu), weight, "", half - 32.0, 1.0);
            app.field(&ids, "strokeWeight", e, |v| json!(v.max(0.0)));
            let more = icons::button(ui, Icon::Dots, 24.0, false, "Advanced stroke");
            egui::Popup::menu(&more).width(240.0).show(|ui| {
                let st = sel.first().and_then(|i| doc.node(*i)).map(|n| n.stroke.clone()).unwrap_or_default();
                ui.label(egui::RichText::new("Stroke style").font(semibold(11.0)));
                if menu_item(ui, "Solid", "") {
                    app.set_props(&ids, json!({"dashes": []}));
                }
                if menu_item(ui, "Dash", "") {
                    app.set_props(&ids, json!({"dashes": [st.weight.max(1.0) * 4.0, st.weight.max(1.0) * 4.0]}));
                }
                ui.separator();
                ui.label(egui::RichText::new("Join").font(semibold(11.0)));
                for (l, v) in [("Miter", "miter"), ("Bevel", "bevel"), ("Round", "round")] {
                    if menu_item(ui, l, "") {
                        app.set_props(&ids, json!({"strokeJoin": v}));
                    }
                }
                ui.separator();
                ui.label(egui::RichText::new("Start / end point").font(semibold(11.0)));
                for c in StrokeCap::ALL {
                    if menu_item(ui, &format!("Start: {}", c.label()), "") {
                        app.set_props(&ids, json!({"startCap": format!("{c:?}")}));
                    }
                }
                for c in StrokeCap::ALL {
                    if menu_item(ui, &format!("End: {}", c.label()), "") {
                        app.set_props(&ids, json!({"endCap": format!("{c:?}")}));
                    }
                }
            });
        });
    }
}

fn paint_row(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, p: &Paint, fills: bool) {
    let t = Tokens::get(ui.ctx());
    let ids = ids.to_vec();
    let cmd = if fills { "fill.set" } else { "stroke.set" };
    row(ui, 32.0, |ui| {
        let w = ui.available_width() - 56.0;
        let (r, _) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::hover());
        ui.painter().rect_filled(r, CornerRadius::same(5), t.field);
        let sw = Rect::from_min_size(pos2(r.min.x + 4.0, r.center().y - 8.0), vec2(16.0, 16.0));
        widgets::paint_swatch(ui.painter(), sw, p);
        let sresp = ui.interact(sw.expand(2.0), Id::new(("swatch", fills, i)), Sense::click());
        if sresp.clicked() {
            app.ui.picker = Some(crate::colorpicker::PickerTarget::Paint { fills, index: i, ids: ids.clone() });
        }
        let text_r = Rect::from_min_max(pos2(sw.max.x + 8.0, r.min.y), pos2(r.max.x - 52.0, r.max.y));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_r));
        match &p.kind {
            PaintKind::Solid { color } => {
                if let Some(h) = widgets::text_field_inline(&mut child, Id::new(("hex", fills, i)), &color.to_hex(), text_r.shrink2(vec2(0.0, 4.0)), regular(11.0))
                    && let Some(c) = figcraft_color::Color::from_hex(&h)
                {
                    app.run(cmd, json!({"ids": ids, "index": i, "color": c.to_css_hex()}));
                }
            }
            other => {
                let label = match other {
                    PaintKind::Gradient(g) => format!("{:?}", g.kind),
                    _ => "Image".into(),
                };
                child.painter().text(pos2(text_r.min.x, r.center().y), Align2::LEFT_CENTER, label, regular(11.0), t.text);
            }
        }
        let op_r = Rect::from_min_max(pos2(r.max.x - 48.0, r.min.y), r.max);
        ui.painter().line_segment([pos2(op_r.min.x, r.min.y + 4.0), pos2(op_r.min.x, r.max.y - 4.0)], Stroke::new(1.0, t.panel));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(op_r));
        let e = widgets::number_field(&mut child, Id::new(("pop", fills, i)), Lead::None, Some((f64::from(p.opacity) * 100.0).round()), "%", 48.0, 1.0);
        match e {
            Some(FieldEdit::Set(v)) => {
                app.run(cmd, json!({"ids": ids, "index": i, "opacity": (v / 100.0).clamp(0.0, 1.0)}));
            }
            Some(FieldEdit::Scrub(v, done)) => {
                let _ = app.session.begin_interaction("Opacity");
                let _ = app.session.execute(cmd, &json!({"ids": ids, "index": i, "opacity": (v / 100.0).clamp(0.0, 1.0)}));
                if done {
                    let _ = app.session.commit_interaction();
                }
            }
            None => {}
        }
        if icons::button(ui, if p.visible { Icon::Eye } else { Icon::EyeOff }, 24.0, false, "Toggle visibility").clicked() {
            app.run(cmd, json!({"ids": ids, "index": i, "visible": !p.visible}));
        }
        if icons::button(ui, Icon::Minus, 24.0, false, "Remove").clicked() {
            app.run(if fills { "fill.remove" } else { "stroke.remove" }, json!({"ids": ids, "index": i}));
        }
    });
}

fn style_picker(app: &mut FigApp, ui: &mut Ui, doc: &Document, ids: &[NodeId], fills: bool) {
    let key = Id::new(("style_menu", fills));
    if !ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false) {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let styles: Vec<(String, String, Vec<Paint>)> = doc
        .styles
        .iter()
        .filter_map(|s| match &s.kind {
            StyleKind::Paint { paints } => Some((s.id.clone(), s.name.clone(), paints.clone())),
            _ => None,
        })
        .collect();
    let mut close = false;
    egui::Frame::NONE.fill(t.hover).inner_margin(egui::Margin::symmetric(16, 8)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Color styles").font(semibold(11.0)));
            if ui.small_button("Create style").clicked() {
                let n = doc.styles.len() + 1;
                app.run("style.create", json!({"type": if fills { "paint" } else { "stroke" }, "name": format!("Color {n}"), "from": ids.first()}));
                close = true;
            }
            if ui.small_button("Close").clicked() {
                close = true;
            }
        });
        for (id, name, paints) in &styles {
            ui.horizontal(|ui| {
                if let Some(p) = paints.first() {
                    widgets::swatch(ui, p, 14.0);
                }
                if ui.selectable_label(false, name).clicked() {
                    app.run("style.apply", json!({"ids": ids, "style": id, "target": if fills { "fill" } else { "stroke" }}));
                    close = true;
                }
            });
        }
    });
    if close {
        ui.data_mut(|d| d.remove::<bool>(key));
    }
}

fn effects(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let t = Tokens::get(ui.ctx());
    let ids = sel.to_vec();
    let list = common(doc, sel, |n| n.effects.clone());
    let empty = list.as_ref().is_none_or(|l| l.is_empty());
    if let Some(_i) = widgets::section_header(ui, "Effects", &[(Icon::Plus, "Add effect")], empty) {
        app.run("effect.add", json!({"ids": ids, "type": "dropShadow"}));
    }
    let Some(list) = list else { return };
    for (i, e) in list.iter().enumerate().rev() {
        let mut open = ui.data(|d| d.get_temp::<bool>(Id::new(("fx_open", i)))).unwrap_or(false);
        row(ui, 32.0, |ui| {
            if icons::button(ui, Icon::Effects, 24.0, open, "Effect settings").clicked() {
                open = !open;
                ui.data_mut(|d| d.insert_temp(Id::new(("fx_open", i)), open));
            }
            let w = ui.available_width() - 56.0;
            let resp = widgets::dropdown(ui, e.label(), w, None);
            egui::Popup::menu(&resp).show(|ui| {
                for (l, ty) in [("Drop shadow", "dropShadow"), ("Inner shadow", "innerShadow"), ("Layer blur", "layerBlur"), ("Background blur", "backgroundBlur"), ("Noise", "noise"), ("Texture", "texture")] {
                    if menu_item(ui, l, "") {
                        let ne = match ty {
                            "dropShadow" => Effect::drop_shadow(),
                            "innerShadow" => Effect::inner_shadow(),
                            "layerBlur" => Effect::layer_blur(),
                            "backgroundBlur" => Effect::background_blur(),
                            "noise" => Effect { kind: EffectKind::Noise { size: 0.5, density: 0.5, color: figcraft_color::Color::new(0.0, 0.0, 0.0, 0.25) }, visible: true },
                            _ => Effect { kind: EffectKind::Texture { size: 0.5, radius: 4.0 }, visible: true },
                        };
                        app.run("effect.set", json!({"ids": ids, "index": i, "effect": ne}));
                        ui.close();
                    }
                }
            });
            if icons::button(ui, if e.visible { Icon::Eye } else { Icon::EyeOff }, 24.0, false, "Toggle").clicked() {
                let mut e2 = e.clone();
                e2.visible = !e2.visible;
                app.run("effect.set", json!({"ids": ids, "index": i, "effect": e2}));
            }
            if icons::button(ui, Icon::Minus, 24.0, false, "Remove").clicked() {
                app.run("effect.remove", json!({"ids": ids, "index": i}));
            }
        });
        if open {
            effect_settings(app, ui, &ids, i, e);
        }
    }
    let _ = t;
}

fn effect_settings(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], i: usize, e: &Effect) {
    let half = (ui.available_width() - PAD * 2.0 - 8.0) / 2.0;
    let set = |app: &mut FigApp, e2: Effect, scrub: Option<bool>| {
        let p = json!({"ids": ids, "index": i, "effect": e2});
        match scrub {
            None => {
                app.run("effect.set", p);
            }
            Some(done) => {
                let _ = app.session.begin_interaction("Effect");
                let _ = app.session.execute("effect.set", &p);
                if done {
                    let _ = app.session.commit_interaction();
                }
            }
        }
    };
    let edit = |app: &mut FigApp, ed: Option<FieldEdit>, f: &dyn Fn(f64) -> Effect| match ed {
        Some(FieldEdit::Set(v)) => set(app, f(v), None),
        Some(FieldEdit::Scrub(v, d)) => set(app, f(v), Some(d)),
        None => {}
    };
    match &e.kind {
        EffectKind::DropShadow(s) | EffectKind::InnerShadow(s) => {
            let inner = matches!(e.kind, EffectKind::InnerShadow(_));
            let mk = |s2: Shadow| Effect { kind: if inner { EffectKind::InnerShadow(s2) } else { EffectKind::DropShadow(s2) }, visible: e.visible };
            row(ui, 32.0, |ui| {
                let ed = widgets::number_field(ui, Id::new(("fx_x", i)), Lead::Text("X"), Some(s.offset.0), "", half, 1.0);
                edit(app, ed, &|v| mk(Shadow { offset: (v, s.offset.1), ..s.clone() }));
                let ed = widgets::number_field(ui, Id::new(("fx_y", i)), Lead::Text("Y"), Some(s.offset.1), "", half, 1.0);
                edit(app, ed, &|v| mk(Shadow { offset: (s.offset.0, v), ..s.clone() }));
            });
            row(ui, 32.0, |ui| {
                let ed = widgets::number_field(ui, Id::new(("fx_b", i)), Lead::Text("Blur"), Some(s.radius), "", half, 1.0);
                edit(app, ed, &|v| mk(Shadow { radius: v.max(0.0), ..s.clone() }));
                let ed = widgets::number_field(ui, Id::new(("fx_s", i)), Lead::Text("Spread"), Some(s.spread), "", half, 1.0);
                edit(app, ed, &|v| mk(Shadow { spread: v, ..s.clone() }));
            });
            row(ui, 32.0, |ui| {
                let p = Paint::solid(s.color);
                if widgets::swatch(ui, &p, 16.0).clicked() {
                    app.ui.picker = Some(crate::colorpicker::PickerTarget::Effect { ids: ids.to_vec(), index: i });
                }
                if let Some(h) = widgets::text_field(ui, Id::new(("fx_c", i)), &s.color.to_hex(), 80.0, "")
                    && let Some(c) = figcraft_color::Color::from_hex(&h)
                {
                    set(app, mk(Shadow { color: c.with_alpha(s.color.a), ..s.clone() }), None);
                }
                let ed = widgets::number_field(ui, Id::new(("fx_a", i)), Lead::None, Some((f64::from(s.color.a) * 100.0).round()), "%", 56.0, 1.0);
                edit(app, ed, &|v| mk(Shadow { color: s.color.with_alpha((v / 100.0) as f32), ..s.clone() }));
            });
            if !inner {
                row(ui, 32.0, |ui| {
                    if widgets::checkbox(ui, s.show_behind, "Show behind transparent areas") {
                        set(app, mk(Shadow { show_behind: !s.show_behind, ..s.clone() }), None);
                    }
                });
            }
        }
        EffectKind::LayerBlur { radius } | EffectKind::BackgroundBlur { radius } => {
            let bg = matches!(e.kind, EffectKind::BackgroundBlur { .. });
            row(ui, 32.0, |ui| {
                let ed = widgets::number_field(ui, Id::new(("fx_r", i)), Lead::Text("Blur"), Some(*radius), "", half * 2.0 + 8.0, 1.0);
                edit(app, ed, &|v| Effect { kind: if bg { EffectKind::BackgroundBlur { radius: v.max(0.0) } } else { EffectKind::LayerBlur { radius: v.max(0.0) } }, visible: e.visible });
            });
        }
        EffectKind::Noise { size, density, color } => {
            row(ui, 32.0, |ui| {
                let ed = widgets::number_field(ui, Id::new(("fx_ns", i)), Lead::Text("Size"), Some(*size), "", half, 0.1);
                edit(app, ed, &|v| Effect { kind: EffectKind::Noise { size: v.max(0.1), density: *density, color: *color }, visible: e.visible });
                let ed = widgets::number_field(ui, Id::new(("fx_nd", i)), Lead::Text("Density"), Some((density * 100.0).round()), "%", half, 1.0);
                edit(app, ed, &|v| Effect { kind: EffectKind::Noise { size: *size, density: (v / 100.0).clamp(0.0, 1.0), color: *color }, visible: e.visible });
            });
        }
        EffectKind::Texture { size, radius } => {
            row(ui, 32.0, |ui| {
                let ed = widgets::number_field(ui, Id::new(("fx_ts", i)), Lead::Text("Size"), Some(*size), "", half, 0.1);
                edit(app, ed, &|v| Effect { kind: EffectKind::Texture { size: v.max(0.1), radius: *radius }, visible: e.visible });
                let ed = widgets::number_field(ui, Id::new(("fx_tr", i)), Lead::Text("Radius"), Some(*radius), "", half, 1.0);
                edit(app, ed, &|v| Effect { kind: EffectKind::Texture { size: *size, radius: v.max(0.0) }, visible: e.visible });
            });
        }
    }
}

fn grids(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let ids = sel.to_vec();
    let list = common(doc, sel, |n| n.grids.clone());
    let empty = list.as_ref().is_none_or(|l| l.is_empty());
    if widgets::section_header(ui, "Layout grid", &[(Icon::Plus, "Add layout grid")], empty).is_some() {
        app.run("grid.add", json!({"ids": ids}));
    }
    let Some(list) = list else { return };
    for (i, g) in list.iter().enumerate() {
        row(ui, 32.0, |ui| {
            let label = match g.kind {
                GridKind::Grid => format!("Grid ({})", g.size),
                GridKind::Columns => format!("{} columns", g.count),
                GridKind::Rows => format!("{} rows", g.count),
            };
            let w = ui.available_width() - 56.0;
            let resp = widgets::dropdown(ui, &label, w, Some(Icon::Grid));
            egui::Popup::menu(&resp).width(220.0).show(|ui| {
                for (l, kind) in [("Grid", GridKind::Grid), ("Columns", GridKind::Columns), ("Rows", GridKind::Rows)] {
                    if menu_item(ui, l, "") {
                        let mut gs = list.clone();
                        if let Some(slot) = gs.get_mut(i) {
                            *slot = match kind {
                                GridKind::Grid => LayoutGrid::default(),
                                GridKind::Columns => LayoutGrid::columns(),
                                GridKind::Rows => LayoutGrid::rows(),
                            };
                        }
                        app.set_props(&ids, json!({"grids": gs}));
                        ui.close();
                    }
                }
            });
            if icons::button(ui, if g.visible { Icon::Eye } else { Icon::EyeOff }, 24.0, false, "Toggle").clicked() {
                let mut gs = list.clone();
                if let Some(slot) = gs.get_mut(i) {
                    slot.visible = !slot.visible;
                }
                app.set_props(&ids, json!({"grids": gs}));
            }
            if icons::button(ui, Icon::Minus, 24.0, false, "Remove").clicked() {
                app.run("grid.remove", json!({"ids": ids, "index": i}));
            }
        });
    }
}

fn export_now(app: &mut FigApp, ids: &[NodeId], format: &str, scale: f64) {
    let name = ids.first().and_then(|i| app.session.doc().ok()?.node(*i).map(|n| n.name.clone())).unwrap_or_else(|| "Export".into());
    let suffix = if (scale - 1.0).abs() > 1e-9 && format != "svg" && format != "pdf" { format!("@{scale}x") } else { String::new() };
    if let Some(p) = app.services.pick_save("Export", &format!("{name}{suffix}.{format}"), &[format]) {
        let ps = p.to_string_lossy().to_string();
        if app.run("file.export", json!({"ids": ids, "format": format, "scale": scale, "path": ps})).is_some() {
            app.toast(format!("Exported {}", p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()));
        }
    }
}

fn export(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let t = Tokens::get(ui.ctx());
    let ids = sel.to_vec();
    let list = common(doc, sel, |n| n.exports.clone()).unwrap_or_default();
    if widgets::section_header(ui, "Export", &[(Icon::Plus, "Add export setting")], list.is_empty()).is_some() {
        let next = match list.last().map(|e| e.size) {
            Some(ExportSize::Scale { value }) => ExportSetting { size: ExportSize::Scale { value: value + 1.0 }, suffix: format!("@{}x", value + 1.0), format: ExportFormat::Png },
            _ => ExportSetting::default(),
        };
        app.run("export.add", json!({"ids": ids, "setting": next}));
    }
    for (i, e) in list.iter().enumerate() {
        row(ui, 32.0, |ui| {
            let scale = match e.size {
                ExportSize::Scale { value } => format!("{value}x"),
                ExportSize::Width { value } => format!("{value}w"),
                ExportSize::Height { value } => format!("{value}h"),
            };
            let resp = widgets::dropdown(ui, &scale, 64.0, None);
            egui::Popup::menu(&resp).show(|ui| {
                for s in [0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0] {
                    if menu_item(ui, &format!("{s}x"), "") {
                        let mut l = list.clone();
                        if let Some(slot) = l.get_mut(i) {
                            slot.size = ExportSize::Scale { value: s };
                        }
                        app.set_props(&ids, json!({"exports": l}));
                        ui.close();
                    }
                }
            });
            if let Some(sfx) = widgets::text_field(ui, Id::new(("exp_sfx", i)), &e.suffix, 64.0, "Suffix") {
                let mut l = list.clone();
                if let Some(slot) = l.get_mut(i) {
                    slot.suffix = sfx;
                }
                app.set_props(&ids, json!({"exports": l}));
            }
            let resp = widgets::dropdown(ui, &e.format.ext().to_uppercase(), 56.0, None);
            egui::Popup::menu(&resp).show(|ui| {
                for f in [ExportFormat::Png, ExportFormat::Jpg, ExportFormat::Svg, ExportFormat::Webp] {
                    if menu_item(ui, &f.ext().to_uppercase(), "") {
                        let mut l = list.clone();
                        if let Some(slot) = l.get_mut(i) {
                            slot.format = f;
                        }
                        app.set_props(&ids, json!({"exports": l}));
                        ui.close();
                    }
                }
            });
            if icons::button(ui, Icon::Minus, 24.0, false, "Remove").clicked() {
                app.run("export.remove", json!({"ids": ids, "index": i}));
            }
        });
    }
    let name = if sel.len() == 1 { sel.first().and_then(|i| doc.node(*i)).map(|n| n.name.clone()).unwrap_or_default() } else { format!("{} layers", sel.len()) };
    row(ui, 32.0, |ui| {
        let w = ui.available_width();
        let (r, resp) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::click());
        ui.painter().rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, t.border), StrokeKind::Inside);
        if resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
        }
        let galley = ui.painter().layout(format!("Export {name}"), regular(11.0), t.text, w - 16.0);
        ui.painter().galley(r.center() - galley.size() / 2.0, galley, t.text);
        if resp.clicked() {
            let e = list.first().cloned().unwrap_or_default();
            let scale = match e.size {
                ExportSize::Scale { value } => value,
                _ => 1.0,
            };
            export_now(app, &ids, e.format.ext(), scale);
        }
    });
}

fn typography(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let t = Tokens::get(ui.ctx());
    let ids: Vec<NodeId> = sel.iter().copied().filter(|i| matches!(doc.node(*i).map(|n| &n.kind), Some(NodeKind::Text(_)))).collect();
    // While editing a range, style edits apply to it.
    let range = figcraft_engine::textedit::selected_range(&app.session);
    let style_of = |n: &Node| -> Option<TextStyle> {
        let td = n.kind.text()?;
        match range {
            Some((id, a, b)) if id == n.id && a != b => Some(td.style_at(a).clone()),
            _ => Some(td.style.clone()),
        }
    };
    let st = common(doc, &ids, style_of).flatten();
    let td = common(doc, &ids, |n| n.kind.text().map(|t| (t.align_h, t.align_v, t.auto_resize))).flatten();
    widgets::section_header(ui, "Typography", &[(Icon::Styles, "Text styles")], false);
    let apply_style = |app: &mut FigApp, key: &str, v: Value| {
        if let Some((id, a, b)) = range
            && a != b
        {
            app.run("text.style", json!({"id": id, "start": a, "end": b, key: v}));
        } else {
            app.set_props(&ids, json!({ key: v }));
        }
    };
    let family = st.as_ref().map(|s| s.font_family.clone()).unwrap_or_else(|| "Mixed".into());
    let fstyle = st.as_ref().map(|s| s.font_style.clone()).unwrap_or_else(|| "Mixed".into());
    row(ui, 32.0, |ui| {
        let w = ui.available_width();
        let resp = widgets::dropdown(ui, &family, w, None);
        egui::Popup::menu(&resp).width(260.0).show(|ui| {
            let q_id = Id::new("font_query");
            let mut q: String = ui.data(|d| d.get_temp(q_id)).unwrap_or_default();
            ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search fonts").desired_width(240.0));
            ui.data_mut(|d| d.insert_temp(q_id, q.clone()));
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for f in figcraft_text::family_names().into_iter().filter(|f| q.is_empty() || f.to_lowercase().contains(&q.to_lowercase())).take(400) {
                    if menu_item(ui, &f, "") {
                        apply_style(app, "fontFamily", json!(f));
                        ui.close();
                    }
                }
            });
        });
    });
    let half = (ui.available_width() - PAD * 2.0 - 8.0) / 2.0;
    row(ui, 32.0, |ui| {
        let resp = widgets::dropdown(ui, &fstyle, half, None);
        let fam = family.clone();
        egui::Popup::menu(&resp).width(200.0).show(|ui| {
            for s in figcraft_text::family_styles(&fam) {
                if menu_item(ui, &s, "") {
                    apply_style(app, "fontStyle", json!(s));
                    ui.close();
                }
            }
        });
        let size = st.as_ref().map(|s| s.font_size);
        let e = widgets::number_field(ui, Id::new("ty_size"), Lead::None, size, "", half, 1.0);
        match e {
            Some(FieldEdit::Set(v)) | Some(FieldEdit::Scrub(v, true)) => apply_style(app, "fontSize", json!(v.max(1.0))),
            Some(FieldEdit::Scrub(v, false)) => {
                let _ = app.session.begin_interaction("Font size");
                let _ = app.session.execute("node.set", &json!({"ids": ids, "props": {"fontSize": v.max(1.0)}}));
            }
            None => {}
        }
        if let Some(FieldEdit::Scrub(_, true)) = e {
            let _ = app.session.commit_interaction();
        }
    });
    row(ui, 32.0, |ui| {
        let lh = st.as_ref().map(|s| s.line_height);
        let lh_label = lh.map(|l| l.label()).unwrap_or_else(|| "Mixed".into());
        if let Some(v) = widgets::text_field(ui, Id::new("ty_lh"), &lh_label, half, "Line height") {
            apply_style(app, "lineHeight", if v.trim().eq_ignore_ascii_case("auto") { json!("auto") } else { json!(v.trim()) });
        }
        let ls = st.as_ref().map(|s| s.letter_spacing.label()).unwrap_or_else(|| "Mixed".into());
        if let Some(v) = widgets::text_field(ui, Id::new("ty_ls"), &ls, half, "Letter spacing") {
            apply_style(app, "letterSpacing", json!(v.trim()));
        }
    });
    row(ui, 32.0, |ui| {
        let items = [(Icon::TextAlignLeft, "Align left"), (Icon::TextAlignCenter, "Align center"), (Icon::TextAlignRight, "Align right"), (Icon::TextAlignJustify, "Justify")];
        let active = td.map(|(h, _, _)| match h {
            TextAlignH::Left => 0,
            TextAlignH::Center => 1,
            TextAlignH::Right => 2,
            TextAlignH::Justified => 3,
        });
        if let Some(i) = widgets::segmented(ui, &items, active, 26.0) {
            app.set_props(&ids, json!({"textAlign": (["left", "center", "right", "justified"][i])}));
        }
        let items = [(Icon::TextTop, "Top"), (Icon::TextMiddle, "Middle"), (Icon::TextBottom, "Bottom")];
        let active = td.map(|(_, v, _)| match v {
            TextAlignV::Top => 0,
            TextAlignV::Middle => 1,
            TextAlignV::Bottom => 2,
        });
        if let Some(i) = widgets::segmented(ui, &items, active, 26.0) {
            app.set_props(&ids, json!({"textAlignVertical": (["top", "middle", "bottom"][i])}));
        }
    });
    row(ui, 32.0, |ui| {
        let items = [(Icon::Hug, "Auto width"), (Icon::LayoutV, "Auto height"), (Icon::Fixed, "Fixed size"), (Icon::Dots, "Truncate text")];
        let active = td.map(|(_, _, r)| match r {
            TextAutoResize::WidthAndHeight => 0,
            TextAutoResize::Height => 1,
            TextAutoResize::None => 2,
            TextAutoResize::Truncate => 3,
        });
        if let Some(i) = widgets::segmented(ui, &items, active, 26.0) {
            app.set_props(&ids, json!({"textAutoResize": (["widthAndHeight", "height", "none", "truncate"][i])}));
        }
        let deco = st.as_ref().map(|s| s.decoration);
        let more = icons::button(ui, Icon::Dots, 24.0, false, "Type details");
        egui::Popup::menu(&more).width(220.0).show(|ui| {
            ui.label(egui::RichText::new("Decoration").font(semibold(11.0)));
            for (l, v) in [("None", "none"), ("Underline", "underline"), ("Strikethrough", "strikethrough")] {
                if menu_item(ui, &format!("{}{l}", if format!("{deco:?}").eq_ignore_ascii_case(&format!("Some({l})")) { "✓ " } else { "" }), "") {
                    apply_style(app, "textDecoration", json!(v));
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("Case").font(semibold(11.0)));
            for (l, v) in [("Original", "original"), ("Uppercase", "upper"), ("Lowercase", "lower"), ("Title case", "title")] {
                if menu_item(ui, l, "") {
                    apply_style(app, "textCase", json!(v));
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("List").font(semibold(11.0)));
            for (l, v) in [("None", "none"), ("Bulleted", "bulleted"), ("Numbered", "numbered")] {
                if menu_item(ui, l, "") {
                    app.set_props(&ids, json!({"listStyle": v}));
                }
            }
        });
    });
    let _ = t;
}

fn prototype(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Ok(ds) = app.session.ds() else { return };
    let doc = ds.doc.clone();
    let sel = ds.selection.clone();
    let page = ds.page;
    if sel.is_empty() {
        let flows = match doc.node(page).map(|n| &n.kind) {
            Some(NodeKind::Page { flows, .. }) => flows.clone(),
            _ => vec![],
        };
        widgets::section_header(ui, "Flow starting points", &[], flows.is_empty());
        for f in &flows {
            row(ui, 32.0, |ui| {
                if icons::button(ui, Icon::Play, 24.0, false, "Present this flow").clicked() {
                    app.run("proto.present", json!({"start": f.start}));
                }
                widgets::label(ui, &f.name, t.text);
            });
        }
        widgets::separator(ui);
        widgets::section_header(ui, "Prototype settings", &[], false);
        let (device, bg) = match doc.node(page).map(|n| &n.kind) {
            Some(NodeKind::Page { prototype, .. }) => (prototype.device.clone(), prototype.background),
            _ => ("None".into(), figcraft_color::Color::BLACK),
        };
        row(ui, 32.0, |ui| {
            let resp = widgets::dropdown(ui, &device, ui.available_width(), None);
            egui::Popup::menu(&resp).show(|ui| {
                for d in ["None", "iPhone 16", "iPhone 16 Pro Max", "Android Compact", "iPad mini", "MacBook Air", "Desktop"] {
                    if menu_item(ui, d, "") {
                        app.run("proto.settings", json!({"device": d}));
                        ui.close();
                    }
                }
            });
        });
        row(ui, 32.0, |ui| {
            widgets::swatch(ui, &Paint::solid(bg), 16.0);
            if let Some(h) = widgets::text_field(ui, Id::new("proto_bg"), &bg.to_hex(), 96.0, "")
                && let Some(c) = figcraft_color::Color::from_hex(&h)
            {
                app.run("proto.settings", json!({"background": c.to_css_hex()}));
            }
        });
        return;
    }
    let ids = sel.clone();
    let first = sel.first().copied().unwrap_or(NodeId(0));
    let interactions = doc.node(first).map(|n| n.interactions.clone()).unwrap_or_default();
    if widgets::section_header(ui, "Interactions", &[(Icon::Plus, "Add interaction")], interactions.is_empty()).is_some() {
        // Default: click → navigate to the next top-level frame.
        let frames = doc.top_frames(page);
        let own = doc.top_level_frame(first);
        let dest = frames.iter().copied().find(|f| Some(*f) != own).or(frames.first().copied());
        match dest {
            Some(d) => {
                app.run("proto.interaction.add", json!({"ids": ids, "dest": d}));
            }
            None => app.toast("Add another frame to navigate to"),
        }
    }
    for (i, it) in interactions.iter().enumerate() {
        let dest_name = it.actions.first().and_then(|a| a.dest()).and_then(|d| doc.node(d)).map(|n| n.name.clone());
        row(ui, 40.0, |ui| {
            ui.vertical(|ui| {
                widgets::label(ui, &it.trigger.label(), t.text);
                let action = it.actions.first().map(|a| a.label()).unwrap_or("None");
                widgets::label(ui, &format!("{action} {}", dest_name.clone().unwrap_or_default()), t.text_secondary);
            });
        });
        row(ui, 32.0, |ui| {
            let frames = doc.top_frames(page);
            let resp = widgets::dropdown(ui, &dest_name.unwrap_or_else(|| "Choose destination".into()), ui.available_width() - 32.0, None);
            let it2 = it.clone();
            egui::Popup::menu(&resp).show(|ui| {
                for f in frames {
                    let name = doc.node(f).map(|n| n.name.clone()).unwrap_or_default();
                    if menu_item(ui, &name, "") {
                        let mut ni = it2.clone();
                        ni.actions = vec![proto::Action::Navigate { dest: f }];
                        app.run("proto.interaction.set", json!({"ids": ids, "index": i, "interaction": ni}));
                        ui.close();
                    }
                }
            });
            if icons::button(ui, Icon::Minus, 24.0, false, "Remove").clicked() {
                app.run("proto.interaction.remove", json!({"ids": ids, "index": i}));
            }
        });
        row(ui, 32.0, |ui| {
            let resp = widgets::dropdown(ui, it.transition.label(), ui.available_width(), None);
            let it2 = it.clone();
            egui::Popup::menu(&resp).show(|ui| {
                for (l, tr) in [
                    ("Instant", proto::Transition::Instant),
                    ("Dissolve", proto::Transition::Dissolve),
                    ("Smart animate", proto::Transition::SmartAnimate),
                    ("Move in", proto::Transition::MoveIn { direction: proto::Direction::Left }),
                    ("Push", proto::Transition::Push { direction: proto::Direction::Left }),
                    ("Slide in", proto::Transition::SlideIn { direction: proto::Direction::Left }),
                ] {
                    if menu_item(ui, l, "") {
                        let mut ni = it2.clone();
                        ni.transition = tr;
                        app.run("proto.interaction.set", json!({"ids": ids, "index": i, "interaction": ni}));
                        ui.close();
                    }
                }
            });
        });
    }
    if doc.top_level_frame(first) == Some(first) {
        widgets::separator(ui);
        widgets::section_header(ui, "Flow starting point", &[(Icon::Plus, "Add starting point")], false);
        row(ui, 32.0, |ui| {
            if ui.button("Set as flow starting point").clicked() {
                app.run("proto.flow.add", json!({"start": first}));
            }
        });
        widgets::separator(ui);
        widgets::section_header(ui, "Scroll behavior", &[], false);
        let ov = doc.node(first).map(|n| n.overflow).unwrap_or_default();
        row(ui, 32.0, |ui| {
            let resp = widgets::dropdown(ui, &format!("Overflow: {ov:?}"), ui.available_width(), None);
            egui::Popup::menu(&resp).show(|ui| {
                for (l, v) in [("No scrolling", "none"), ("Horizontal", "horizontal"), ("Vertical", "vertical"), ("Both directions", "both")] {
                    if menu_item(ui, l, "") {
                        app.set_props(&ids, json!({"overflow": v}));
                        ui.close();
                    }
                }
            });
        });
    }
}

/// CSS for a node (Inspect / Copy as CSS).
pub fn css_for(doc: &Document, id: NodeId) -> String {
    let Some(n) = doc.node(id) else { return String::new() };
    let mut out = format!("/* {} */\n", n.name);
    out.push_str(&format!("width: {}px;\nheight: {}px;\n", crate::canvas::fmt(n.width), crate::canvas::fmt(n.height)));
    if let Some(l) = &n.layout {
        out.push_str("display: flex;\n");
        out.push_str(&format!("flex-direction: {};\n", if l.mode == LayoutMode::Vertical { "column" } else { "row" }));
        if l.wrap {
            out.push_str("flex-wrap: wrap;\n");
        }
        out.push_str(&format!("gap: {}px;\n", crate::canvas::fmt(l.gap)));
        let [pt, pr, pb, pl] = l.padding;
        out.push_str(&format!("padding: {}px {}px {}px {}px;\n", crate::canvas::fmt(pt), crate::canvas::fmt(pr), crate::canvas::fmt(pb), crate::canvas::fmt(pl)));
        out.push_str(&format!(
            "justify-content: {};\nalign-items: {};\n",
            match l.main_align {
                MainAlign::Min => "flex-start",
                MainAlign::Center => "center",
                MainAlign::Max => "flex-end",
                MainAlign::SpaceBetween => "space-between",
            },
            match l.cross_align {
                CrossAlign::Min => "flex-start",
                CrossAlign::Center => "center",
                CrossAlign::Max => "flex-end",
                CrossAlign::Baseline => "baseline",
            }
        ));
    }
    if let Some(r) = n.corners.uniform().filter(|r| *r > 0.0) {
        out.push_str(&format!("border-radius: {}px;\n", crate::canvas::fmt(r)));
    } else if n.corners.radii.iter().any(|r| *r > 0.0) {
        let [a, b, c, d] = n.corners.radii;
        out.push_str(&format!("border-radius: {}px {}px {}px {}px;\n", a, b, c, d));
    }
    if n.opacity < 1.0 {
        out.push_str(&format!("opacity: {};\n", n.opacity));
    }
    for p in n.fills.iter().filter(|p| p.visible) {
        match &p.kind {
            PaintKind::Solid { color } => {
                let key = if matches!(n.kind, NodeKind::Text(_)) { "color" } else { "background" };
                out.push_str(&format!("{key}: {};\n", color.with_alpha(color.a * p.opacity).to_css()));
            }
            PaintKind::Gradient(g) => {
                let stops: Vec<String> = g.sorted_stops().iter().map(|s| format!("{} {}%", s.color.to_css(), (s.position * 100.0).round())).collect();
                out.push_str(&format!("background: linear-gradient({});\n", stops.join(", ")));
            }
            PaintKind::Image(_) => out.push_str("background: url(image.png) center / cover;\n"),
        }
    }
    if n.has_visible_stroke()
        && let Some(c) = n.strokes.first().and_then(|p| p.solid_color())
    {
        out.push_str(&format!("border: {}px solid {};\n", crate::canvas::fmt(n.stroke.weight), c.to_css()));
    }
    let shadows: Vec<String> = n
        .effects
        .iter()
        .filter(|e| e.visible)
        .filter_map(|e| match &e.kind {
            EffectKind::DropShadow(s) => Some(format!("{}px {}px {}px {}px {}", s.offset.0, s.offset.1, s.radius, s.spread, s.color.to_css())),
            EffectKind::InnerShadow(s) => Some(format!("inset {}px {}px {}px {}px {}", s.offset.0, s.offset.1, s.radius, s.spread, s.color.to_css())),
            _ => None,
        })
        .collect();
    if !shadows.is_empty() {
        out.push_str(&format!("box-shadow: {};\n", shadows.join(", ")));
    }
    for e in n.effects.iter().filter(|e| e.visible) {
        match e.kind {
            EffectKind::LayerBlur { radius } => out.push_str(&format!("filter: blur({}px);\n", radius / 2.0)),
            EffectKind::BackgroundBlur { radius } => out.push_str(&format!("backdrop-filter: blur({}px);\n", radius / 2.0)),
            _ => {}
        }
    }
    if let NodeKind::Text(t) = &n.kind {
        out.push_str(&format!("font-family: \"{}\";\nfont-size: {}px;\nfont-weight: {};\n", t.style.font_family, t.style.font_size, t.style.weight()));
        if t.style.italic() {
            out.push_str("font-style: italic;\n");
        }
        match t.style.line_height {
            LineHeight::Pixels(v) => out.push_str(&format!("line-height: {v}px;\n")),
            LineHeight::Percent(p) => out.push_str(&format!("line-height: {p}%;\n")),
            LineHeight::Auto => {}
        }
        if let LetterSpacing::Pixels(v) = t.style.letter_spacing
            && v != 0.0
        {
            out.push_str(&format!("letter-spacing: {v}px;\n"));
        }
        if let LetterSpacing::Percent(p) = t.style.letter_spacing
            && p != 0.0
        {
            out.push_str(&format!("letter-spacing: {}em;\n", p / 100.0));
        }
    }
    out
}

fn inspect(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Ok(ds) = app.session.ds() else { return };
    let doc = ds.doc.clone();
    let sel = ds.selection.clone();
    widgets::section_header(ui, "Inspect", &[], false);
    let Some(first) = sel.first().copied() else {
        row(ui, 32.0, |ui| widgets::label(ui, "Select a layer to see its code and measurements", t.text_secondary));
        return;
    };
    if let Some(b) = doc.world_bounds(first) {
        row(ui, 32.0, |ui| {
            widgets::label(ui, &format!("{} × {}   at {}, {}", crate::canvas::fmt(b.width()), crate::canvas::fmt(b.height()), crate::canvas::fmt(b.x0), crate::canvas::fmt(b.y0)), t.text);
        });
    }
    widgets::separator(ui);
    widgets::section_header(ui, "CSS", &[(Icon::Export, "Copy")], false);
    let css = css_for(&doc, first);
    egui::Frame::NONE.fill(t.field).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::same(10)).outer_margin(egui::Margin::symmetric(16, 0)).show(ui, |ui| {
        ui.label(egui::RichText::new(&css).monospace().color(t.text));
    });
    ui.add_space(8.0);
    row(ui, 32.0, |ui| {
        if ui.button("Copy CSS").clicked() {
            app.services.clipboard_set_text(&css);
            app.toast("Copied CSS");
        }
    });
}
