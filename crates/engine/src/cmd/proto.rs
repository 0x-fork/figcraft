//! Prototype commands: interactions, flows, settings, presenting.

use figcraft_doc::proto::*;
use figcraft_doc::*;
use serde_json::{Value, json};

use super::{CommandSpec, ok};
use crate::params::*;
use crate::{Result, Session, bad, spec};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "proto.interaction.add",
            "Add interaction",
            "{id?, trigger? {type: onClick|onHover|onPress|onDrag|afterDelay|mouseEnter|mouseLeave|keyPress,..}, action {type: navigate|back|scrollTo|openOverlay|swapOverlay|closeOverlay|openUrl|changeTo|setVariable, dest?, url?}, transition? {type: instant|dissolve|smartAnimate|moveIn|push|slideIn,..}, durationMs?, easing?}  — or {id, dest} for a click-navigate",
            true,
            add
        ),
        spec!("proto.interaction.remove", "Remove interaction", "{id?, index}", true, |s, p| {
            let k = req_num("proto.interaction.remove", p, "index")? as usize;
            for i in req_targets("proto.interaction.remove", s, p)? {
                let n = s.doc_mut()?.get_mut(i)?;
                if k < n.interactions.len() {
                    n.interactions.remove(k);
                }
            }
            ok()
        }),
        spec!("proto.interaction.set", "Edit interaction", "{id?, index, interaction {..}}", true, |s, p| {
            let k = req_num("proto.interaction.set", p, "index")? as usize;
            let it: Interaction =
                parse("proto.interaction.set", p, "interaction")?.ok_or_else(|| bad("proto.interaction.set", "`interaction` required"))?;
            for i in req_targets("proto.interaction.set", s, p)? {
                let n = s.doc_mut()?.get_mut(i)?;
                if let Some(slot) = n.interactions.get_mut(k) {
                    *slot = it.clone();
                }
            }
            ok()
        }),
        spec!("proto.flow.add", "Add flow starting point", "{start? (frame id; default selection), name?}", true, |s, p| {
            let start = id(p, "start").or_else(|| s.selection().first().copied()).ok_or_else(|| bad("proto.flow.add", "select a top-level frame"))?;
            let page = s.page()?;
            let doc = s.doc_mut()?;
            let count = match &doc.get(page)?.kind {
                NodeKind::Page { flows, .. } => flows.len(),
                _ => 0,
            };
            let name = string(p, "name").unwrap_or_else(|| format!("Flow {}", count + 1));
            if let NodeKind::Page { flows, .. } = &mut doc.get_mut(page)?.kind {
                flows.retain(|f| f.start != start);
                flows.push(Flow { name, start, description: String::new() });
            }
            ok()
        }),
        spec!("proto.flow.remove", "Remove flow", "{start}", true, |s, p| {
            let start = req_id("proto.flow.remove", p, "start")?;
            let page = s.page()?;
            if let NodeKind::Page { flows, .. } = &mut s.doc_mut()?.get_mut(page)?.kind {
                flows.retain(|f| f.start != start);
            }
            ok()
        }),
        spec!("proto.settings", "Prototype settings", "{device?, background?}", true, |s, p| {
            let page = s.page()?;
            let bg = color(p, "background");
            let dev = string(p, "device");
            if let NodeKind::Page { prototype, .. } = &mut s.doc_mut()?.get_mut(page)?.kind {
                if let Some(c) = bg {
                    prototype.background = c;
                }
                if let Some(d) = dev {
                    prototype.device = d;
                }
            }
            ok()
        }),
        spec!("proto.present", "Present", "{start? (frame id)}", false, |s, p| {
            let start = id(p, "start").or_else(|| {
                let ds = s.ds().ok()?;
                ds.selection
                    .first()
                    .and_then(|i| ds.doc.top_level_frame(*i))
                    .or_else(|| match &ds.doc.node(ds.page)?.kind {
                        NodeKind::Page { flows, .. } => flows.first().map(|f| f.start),
                        _ => None,
                    })
                    .or_else(|| ds.doc.top_frames(ds.page).first().copied())
            });
            let start = start.ok_or_else(|| bad("proto.present", "no frame to present"))?;
            s.request_ui("present", json!({"start": start}));
            Ok(json!({"start": start}))
        }),
    ]
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "proto.interaction.add";
    let it = if let Some(d) = id(p, "dest") {
        Interaction {
            trigger: Trigger::OnClick,
            actions: vec![Action::Navigate { dest: d }],
            transition: Transition::Instant,
            duration_ms: 300,
            easing: Easing::EaseOut,
        }
    } else {
        let trigger: Trigger = parse(cmd, p, "trigger")?.unwrap_or_default();
        let action: Action = parse(cmd, p, "action")?.ok_or_else(|| bad(cmd, "`action` required"))?;
        let transition: Transition = parse(cmd, p, "transition")?.unwrap_or_default();
        let easing: Easing = parse(cmd, p, "easing")?.unwrap_or_default();
        Interaction { trigger, actions: vec![action], transition, duration_ms: num_or(p, "durationMs", 300.0).clamp(0.0, 60_000.0) as u32, easing }
    };
    for i in req_targets(cmd, s, p)? {
        s.doc_mut()?.get_mut(i)?.interactions.push(it.clone());
    }
    ok()
}
