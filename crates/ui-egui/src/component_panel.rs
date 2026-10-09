//! Component and instance sections of the Design panel.
#![allow(unused_imports)]

use egui::{Align2, Color32, CornerRadius, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_color::{BlendMode, Paint, PaintKind};
use figcraft_doc::*;
use serde_json::{Value, json};

use crate::FigApp;
use crate::icons::{self, Icon};
use crate::right::{common, menu_item, row};
use crate::theme::{ACCENT, Tokens, regular, semibold};
use crate::widgets::{self, FIELD_H, FieldEdit, Lead, PAD};

pub fn instance_section(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let t = Tokens::get(ui.ctx());
    // Instance row.
    if sel.len() == 1
        && let Some(n) = sel.first().and_then(|i| doc.node(*i))
        && let NodeKind::Instance { component, props } = &n.kind
    {
        let comp_name = doc.node(*component).map(|c| c.name.clone()).unwrap_or_else(|| "Missing component".into());
        let set =
            doc.node(*component).and_then(|c| c.parent).filter(|p| doc.node(*p).is_some_and(|pn| matches!(pn.kind, NodeKind::ComponentSet { .. })));
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
