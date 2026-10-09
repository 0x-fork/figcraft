//! The Prototype tab.
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

pub fn prototype(app: &mut FigApp, ui: &mut Ui) {
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
