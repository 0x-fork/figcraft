//! Text menu and text editing commands.

use figcraft_doc::*;
use serde_json::{Map, Value, json};

use super::{CommandSpec, ok};
use crate::params::*;
use crate::{Result, Session, bad, spec, textedit};

/// Apply a style change to the edited range, or to whole text layers in the selection.
fn style(s: &mut Session, p: &Value, f: impl Fn(&mut TextStyle)) -> Result<Value> {
    if let Some((id, a, b)) = textedit::selected_range(s) {
        let ds = s.ds_mut()?;
        let before = ds.doc.clone();
        let doc = ds.doc_mut();
        if let NodeKind::Text(t) = &mut doc.get_mut(id)?.kind {
            if a == b {
                t.restyle(0, t.char_count(), &f);
            } else {
                t.restyle(a, b, &f);
            }
        }
        crate::normalize(doc, &before);
        return ok();
    }
    let ids = req_targets("text", s, p)?;
    let doc = s.doc_mut()?;
    for i in ids {
        for d in doc.subtree(i) {
            if let Some(NodeKind::Text(t)) = doc.node_mut(d).map(|n| &mut n.kind) {
                let n = t.char_count();
                t.restyle(0, n, &f);
            }
        }
    }
    ok()
}

fn para(s: &mut Session, p: &Value, f: impl Fn(&mut TextData)) -> Result<Value> {
    let ids = match textedit::selected_range(s) {
        Some((id, _, _)) => vec![id],
        None => req_targets("text", s, p)?,
    };
    let ds = s.ds_mut()?;
    let before = ds.doc.clone();
    let doc = ds.doc_mut();
    for i in ids {
        for d in doc.subtree(i) {
            if let Some(NodeKind::Text(t)) = doc.node_mut(d).map(|n| &mut n.kind) {
                f(t);
            }
        }
    }
    crate::normalize(doc, &before);
    ok()
}

/// Whether every targeted char has `pred` (to decide toggle direction).
fn all_have(s: &Session, p: &Value, pred: impl Fn(&TextStyle) -> bool) -> bool {
    let Ok(doc) = s.doc() else { return false };
    if let Some((id, a, b)) = textedit::selected_range(s) {
        if let Some(NodeKind::Text(t)) = doc.node(id).map(|n| &n.kind) {
            let (a, b) = if a == b { (0, t.char_count()) } else { (a, b) };
            return (a..b.max(a + 1)).all(|i| pred(t.style_at(i)));
        }
        return false;
    }
    targets(s, p).iter().all(|i| matches!(doc.node(*i).map(|n| &n.kind), Some(NodeKind::Text(t)) if t.spans().iter().all(|(_, _, st)| pred(st))))
}

fn toggle_bold(s: &mut Session, p: &Value) -> Result<Value> {
    let bold = all_have(s, p, |st| st.weight() >= 700);
    style(s, p, |st| {
        let italic = st.italic();
        let base = if bold { "Regular" } else { "Bold" };
        st.font_style = if italic { if base == "Regular" { "Italic".into() } else { format!("{base} Italic") } } else { base.into() };
    })
}

fn toggle_italic(s: &mut Session, p: &Value) -> Result<Value> {
    let italic = all_have(s, p, TextStyle::italic);
    style(s, p, |st| {
        let w = super::props::weight_name(st.weight());
        st.font_style = if italic {
            w.to_string()
        } else if w == "Regular" {
            "Italic".into()
        } else {
            format!("{w} Italic")
        };
    })
}

fn step_weight(s: &mut Session, p: &Value, up: bool) -> Result<Value> {
    style(s, p, |st| {
        let w = st.weight() as i32 + if up { 100 } else { -100 };
        let w = w.clamp(100, 900) as u16;
        let name = super::props::weight_name(w);
        st.font_style = if st.italic() { if name == "Regular" { "Italic".into() } else { format!("{name} Italic") } } else { name.into() };
    })
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!("text.bold", "Bold", "{ids?}", true, toggle_bold),
        spec!("text.italic", "Italic", "{ids?}", true, toggle_italic),
        spec!("text.underline", "Underline", "{ids?}", true, |s, p| {
            let on = all_have(s, p, |st| st.decoration == TextDecoration::Underline);
            style(s, p, |st| st.decoration = if on { TextDecoration::None } else { TextDecoration::Underline })
        }),
        spec!("text.strikethrough", "Strikethrough", "{ids?}", true, |s, p| {
            let on = all_have(s, p, |st| st.decoration == TextDecoration::Strikethrough);
            style(s, p, |st| st.decoration = if on { TextDecoration::None } else { TextDecoration::Strikethrough })
        }),
        spec!("text.link", "Create link", "{url, ids?}", true, |s, p| {
            let url = req_string("text.link", p, "url")?;
            style(s, p, |st| st.link = Some(url.clone()))
        }),
        spec!("text.bulletList", "Bulleted list", "{ids?}", true, |s, p| para(s, p, |t| t.list =
            if t.list == ListStyle::Bulleted { ListStyle::None } else { ListStyle::Bulleted })),
        spec!("text.numberList", "Numbered list", "{ids?}", true, |s, p| para(s, p, |t| t.list =
            if t.list == ListStyle::Numbered { ListStyle::None } else { ListStyle::Numbered })),
        spec!("text.alignLeft", "Align left", "{ids?}", true, |s, p| para(s, p, |t| t.align_h = TextAlignH::Left)),
        spec!("text.alignCenter", "Align center", "{ids?}", true, |s, p| para(s, p, |t| t.align_h = TextAlignH::Center)),
        spec!("text.alignRight", "Align right", "{ids?}", true, |s, p| para(s, p, |t| t.align_h = TextAlignH::Right)),
        spec!("text.alignJustify", "Justify", "{ids?}", true, |s, p| para(s, p, |t| t.align_h = TextAlignH::Justified)),
        spec!("text.sizeUp", "Increase font size", "{ids?}", true, |s, p| style(s, p, |st| st.font_size = (st.font_size + 1.0).min(5000.0))),
        spec!("text.sizeDown", "Decrease font size", "{ids?}", true, |s, p| style(s, p, |st| st.font_size = (st.font_size - 1.0).max(1.0))),
        spec!("text.weightUp", "Increase font weight", "{ids?}", true, |s, p| step_weight(s, p, true)),
        spec!("text.weightDown", "Decrease font weight", "{ids?}", true, |s, p| step_weight(s, p, false)),
        spec!("text.lineHeightUp", "Increase line height", "{ids?}", true, |s, p| {
            style(s, p, |st| st.line_height = LineHeight::Pixels(st.line_height.resolve(st.font_size, 1.21).round() + 1.0))
        }),
        spec!("text.lineHeightDown", "Decrease line height", "{ids?}", true, |s, p| {
            style(s, p, |st| st.line_height = LineHeight::Pixels((st.line_height.resolve(st.font_size, 1.21).round() - 1.0).max(0.0)))
        }),
        spec!("text.trackingUp", "Increase letter spacing", "{ids?}", true, |s, p| {
            style(s, p, |st| {
                st.letter_spacing = LetterSpacing::Percent(match st.letter_spacing {
                    LetterSpacing::Percent(v) => v + 1.0,
                    LetterSpacing::Pixels(v) => v / st.font_size.max(1.0) * 100.0 + 1.0,
                })
            })
        }),
        spec!("text.trackingDown", "Decrease letter spacing", "{ids?}", true, |s, p| {
            style(s, p, |st| {
                st.letter_spacing = LetterSpacing::Percent(match st.letter_spacing {
                    LetterSpacing::Percent(v) => v - 1.0,
                    LetterSpacing::Pixels(v) => v / st.font_size.max(1.0) * 100.0 - 1.0,
                })
            })
        }),
        spec!("text.caseOriginal", "Original case", "{ids?}", true, |s, p| style(s, p, |st| st.case = TextCase::Original)),
        spec!("text.caseUpper", "Uppercase", "{ids?}", true, |s, p| style(s, p, |st| st.case = TextCase::Upper)),
        spec!("text.caseLower", "Lowercase", "{ids?}", true, |s, p| style(s, p, |st| st.case = TextCase::Lower)),
        spec!("text.caseTitle", "Title case", "{ids?}", true, |s, p| style(s, p, |st| st.case = TextCase::Title)),
        spec!("text.set", "Set text", "{id?, characters}", true, |s, p| {
            let c = string(p, "characters").or_else(|| string(p, "text")).ok_or_else(|| bad("text.set", "`characters` required"))?;
            let ids = req_targets("text.set", s, p)?;
            let doc = s.doc_mut()?;
            for i in ids {
                let mut m = Map::new();
                m.insert("characters".into(), json!(c));
                super::props::apply("text.set", doc, i, &m)?;
            }
            ok()
        }),
        spec!(
            "text.style",
            "Style text range",
            "{id?, start?, end?, fontFamily?, fontStyle?, fontWeight?, fontSize?, lineHeight?, letterSpacing?, textDecoration?, textCase?, hyperlink?, textFills?}",
            true,
            |s, p| {
                let ids = req_targets("text.style", s, p)?;
                let m = p.as_object().cloned().unwrap_or_default();
                let doc = s.doc_mut()?;
                for i in ids {
                    let n = doc.get_mut(i)?;
                    let NodeKind::Text(t) = &mut n.kind else { return Err(bad("text.style", format!("{i} is not text"))) };
                    let len = t.char_count();
                    let a = num(p, "start").map(|v| v.max(0.0) as usize).unwrap_or(0).min(len);
                    let b = num(p, "end").map(|v| v.max(0.0) as usize).unwrap_or(len).min(len);
                    for (k, v) in &m {
                        if matches!(k.as_str(), "id" | "ids" | "start" | "end") {
                            continue;
                        }
                        super::props::apply_text_style("text.style", t, a, b, k, v)?;
                    }
                }
                ok()
            }
        ),
        spec!("text.edit.begin", "Edit text", "{id, caret? (char index; default select all)}", false, |s, p| {
            let id = req_id("text.edit.begin", p, "id")?;
            let caret = num(p, "caret").map(|c| c.max(0.0) as usize);
            textedit::begin(s, id, caret, false)?;
            ok()
        }),
        spec!("text.edit.insert", "Type text", "{text}", true, |s, p| {
            let t = string(p, "text").unwrap_or_default();
            textedit::insert(s, &t)?;
            ok()
        }),
        spec!(
            "text.edit.key",
            "Text key",
            "{key: Backspace|Delete|Left|Right|Up|Down|Home|End|Enter|Escape|SelectAll, shift?, alt?, cmd?}",
            true,
            |s, p| {
                let k = req_string("text.edit.key", p, "key")?;
                textedit::key(s, &k, boolean(p, "shift").unwrap_or(false), boolean(p, "alt").unwrap_or(false), boolean(p, "cmd").unwrap_or(false))?;
                ok()
            }
        ),
        spec!("text.edit.caret", "Set caret", "{index, extend?}", false, |s, p| {
            let i = req_num("text.edit.caret", p, "index")?.max(0.0) as usize;
            textedit::set_caret(s, i, boolean(p, "extend").unwrap_or(false))?;
            ok()
        }),
        spec!("text.edit.end", "Stop editing text", "{}", false, |s, _| {
            textedit::end(s)?;
            ok()
        }),
        spec!("text.fonts", "List fonts", "{family? (list its styles)}", false, |_, p| {
            Ok(match string(p, "family") {
                Some(f) => json!(figcraft_text::family_styles(&f)),
                None => json!(figcraft_text::family_names()),
            })
        }),
    ]
}
