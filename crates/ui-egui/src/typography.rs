//! Typography section of the Design panel.
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

pub fn typography(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
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
                for f in figcraft_text::family_names().into_iter().filter(|f| q.is_empty() || f.to_lowercase().contains(&q.to_lowercase())).take(400)
                {
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
        let items = [
            (Icon::TextAlignLeft, "Align left"),
            (Icon::TextAlignCenter, "Align center"),
            (Icon::TextAlignRight, "Align right"),
            (Icon::TextAlignJustify, "Justify"),
        ];
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
