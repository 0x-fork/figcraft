//! Component commands.

use figcraft_doc::*;
use serde_json::{Value, json};

use super::edit::{roots, z_sorted};
use super::{CommandSpec, ok};
use crate::components::*;
use crate::params::*;
use crate::{EngineError, Result, Session, bad, spec};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!("component.create", "Create component", "{ids?}  (one frame/group becomes a component; several layers are wrapped in one)", true, create),
        spec!("component.createMultiple", "Create multiple components", "{ids?}", true, |s, p| {
            let ids = req_targets("component.createMultiple", s, p)?;
            let mut out = Vec::new();
            for i in ids {
                let r = create(s, &json!({"ids": [i]}))?;
                out.push(r["id"].clone());
            }
            s.set_selection(out.iter().filter_map(id_of).collect());
            Ok(json!({"ids": out}))
        }),
        spec!("component.combineVariants", "Combine as variants", "{ids? (components)}", true, combine),
        spec!("instance.create", "Create instance", "{component, x?, y?, parent?}", true, |s, p| {
            let c = req_id("instance.create", p, "component")?;
            let page = s.page()?;
            let parent = id(p, "parent").unwrap_or(page);
            let doc = s.doc_mut()?;
            let (x, y) = match (num(p, "x"), num(p, "y")) {
                (Some(x), Some(y)) => (x, y),
                _ => doc.world_bounds(c).map(|b| (b.x1 + 40.0, b.y0)).unwrap_or((0.0, 0.0)),
            };
            let i = create_instance(doc, c, parent, x, y)?;
            s.set_selection(vec![i]);
            Ok(json!({"id": i}))
        }),
        spec!("component.detach", "Detach instance", "{ids?}", true, detach),
        spec!("component.resetInstance", "Reset all changes", "{ids?}", true, |s, p| {
            for i in req_targets("component.resetInstance", s, p)? {
                let doc = s.doc_mut()?;
                let inst = owning_instance(doc, i).unwrap_or(i);
                for d in doc.subtree(inst) {
                    if let Some(n) = doc.node_mut(d) {
                        n.overrides.clear();
                    }
                }
                sync_instance(doc, inst);
            }
            ok()
        }),
        spec!("component.goToMain", "Go to main component", "{id?}", false, |s, p| {
            let i = targets(s, p).first().copied().ok_or_else(|| bad("component.goToMain", "select an instance"))?;
            let doc = s.doc()?;
            let inst = owning_instance(doc, i).ok_or_else(|| bad("component.goToMain", "not an instance"))?;
            let Some(NodeKind::Instance { component, .. }) = doc.node(inst).map(|n| n.kind.clone()) else { return Err(bad("component.goToMain", "not an instance")) };
            if !doc.contains(component) {
                return Err(EngineError::Other("the main component was deleted".into()));
            }
            super::edit::select_and_page(s, vec![component])?;
            let ds = s.ds_mut()?;
            if let Some(b) = ds.doc.world_bounds(component) {
                ds.view.fit(b, 2.0);
            }
            Ok(json!({"id": component}))
        }),
        spec!("component.pushChanges", "Push changes to main component", "{id?}", true, push_changes),
        spec!("component.restoreMain", "Restore main component", "{id?}", true, |_s, _p| {
            Err(EngineError::Disabled("component.restoreMain".into(), "the main component still exists".into()))
        }),
        spec!("instance.swap", "Swap instance", "{ids?, component}", true, |s, p| {
            let c = req_id("instance.swap", p, "component")?;
            for i in req_targets("instance.swap", s, p)? {
                if let NodeKind::Instance { component, .. } = &mut s.doc_mut()?.get_mut(i)?.kind {
                    *component = c;
                }
            }
            ok()
        }),
        spec!("instance.setProperties", "Set instance properties", "{ids?, properties {name: value}} (variant properties switch variants)", true, set_props),
        spec!("component.addProperty", "Add component property", "{id? (component or set), name, type: boolean|text|instanceSwap|variant, default, layer? (node bound to it)}", true, add_prop),
        spec!("component.list", "List components", "{}", false, |s, _| {
            let doc = s.doc()?;
            Ok(Value::Array(
                doc.nodes()
                    .filter(|n| matches!(n.kind, NodeKind::Component { .. } | NodeKind::ComponentSet { .. }))
                    .map(|n| json!({"id": n.id, "name": n.name, "type": n.kind.type_name(), "page": doc.page_of(n.id), "instances": instances_of(doc, n.id).len()}))
                    .collect(),
            ))
        }),
    ]
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("component.create", s, p)?;
    let doc = s.doc_mut()?;
    let ids = z_sorted(doc, &roots(doc, &ids));
    let target = if ids.len() == 1 && ids.first().and_then(|i| doc.node(*i)).is_some_and(|n| matches!(n.kind, NodeKind::Frame | NodeKind::Group)) {
        ids.first().copied().ok_or_else(|| bad("component.create", "nothing selected"))?
    } else {
        // Wrap the layers in a frame first (like Figma).
        let r = super::structure::specs()
            .into_iter()
            .find(|x| x.id == "object.frameSelection")
            .map(|x| (x.run)(s, &json!({"ids": ids})))
            .ok_or_else(|| EngineError::Other("frame selection unavailable".into()))??;
        let fid = id_of(&r["id"]).ok_or_else(|| EngineError::Other("wrap failed".into()))?;
        let doc = s.doc_mut()?;
        let n = doc.get_mut(fid)?;
        n.fills.clear();
        fid
    };
    let doc = s.doc_mut()?;
    let n = doc.get_mut(target)?;
    let was_group = matches!(n.kind, NodeKind::Group);
    n.kind = NodeKind::Component { description: String::new(), props: Default::default() };
    if was_group {
        n.fills.clear();
        n.clip = false;
        n.blend = figcraft_color::BlendMode::PassThrough;
    }
    if ids.len() == 1 && was_group {
        // Keep the group's name; frames keep theirs too.
    }
    s.set_selection(vec![target]);
    Ok(json!({"id": target}))
}

fn combine(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("component.combineVariants", s, p)?;
    let doc = s.doc_mut()?;
    if !ids.iter().all(|i| doc.node(*i).is_some_and(|n| matches!(n.kind, NodeKind::Component { .. }))) {
        return Err(bad("component.combineVariants", "select components only"));
    }
    let ids = z_sorted(doc, &ids);
    let top = *ids.last().ok_or_else(|| bad("component.combineVariants", "nothing selected"))?;
    let parent = doc.get(top)?.parent.ok_or_else(|| bad("component.combineVariants", "page"))?;
    let bounds = doc.bounds_of(&ids).unwrap_or_default();
    let pw = doc.world_transform(parent);
    let local = figcraft_geom::transform_rect_bbox(pw.inverse(), bounds.inflate(20.0, 20.0));
    let base = ids.first().and_then(|i| doc.node(*i)).map(|n| n.name.split('/').next().unwrap_or("Component").trim().to_string()).unwrap_or_else(|| "Component".into());
    let set = doc.alloc_id();
    let mut n = Node::new(set, NodeKind::ComponentSet { description: String::new() }, base, local.width(), local.height());
    n.transform = figcraft_geom::Affine::translate((local.x0, local.y0));
    n.fills.clear();
    doc.insert(n, parent, doc.index_in_parent(top).map(|i| i + 1))?;
    for (k, i) in ids.iter().enumerate() {
        doc.reparent(*i, set, None)?;
        // Name variants "Property 1=Variant N" unless already in prop=value form.
        let node = doc.get_mut(*i)?;
        if !node.name.contains('=') {
            node.name = format!("Property 1={}", if k == 0 { "Default".to_string() } else { format!("Variant{}", k + 1) });
        }
    }
    s.set_selection(vec![set]);
    Ok(json!({"id": set}))
}

fn detach(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("component.detach", s, p)?;
    let doc = s.doc_mut()?;
    let mut out = Vec::new();
    for i in ids {
        let inst = if matches!(doc.get(i)?.kind, NodeKind::Instance { .. }) { i } else { continue };
        for d in doc.subtree(inst) {
            if let Some(n) = doc.node_mut(d) {
                n.source = None;
                n.overrides.clear();
            }
        }
        let n = doc.get_mut(inst)?;
        n.kind = NodeKind::Frame;
        out.push(inst);
    }
    s.set_selection(out.clone());
    Ok(json!({"ids": out}))
}

fn push_changes(s: &mut Session, p: &Value) -> Result<Value> {
    let i = targets(s, p).first().copied().ok_or_else(|| bad("component.pushChanges", "select an instance"))?;
    let doc = s.doc_mut()?;
    let inst = owning_instance(doc, i).ok_or_else(|| bad("component.pushChanges", "not an instance"))?;
    let Some(NodeKind::Instance { component, .. }) = doc.node(inst).map(|n| n.kind.clone()) else { return ok() };
    // Copy every overridden layer's values to its source in the main.
    for d in doc.subtree(inst) {
        let Some(n) = doc.node(d).cloned() else { continue };
        let Some(src) = (if d == inst { Some(component) } else { n.source }) else { continue };
        if n.overrides.is_empty() || !doc.contains(src) {
            continue;
        }
        let Some(mut target) = doc.node(src).cloned() else { continue };
        for g in &n.overrides {
            match g.as_str() {
                "fills" => target.fills = n.fills.clone(),
                "strokes" => {
                    target.strokes = n.strokes.clone();
                    target.stroke = n.stroke.clone();
                }
                "effects" => target.effects = n.effects.clone(),
                "opacity" => target.opacity = n.opacity,
                "visible" => target.visible = n.visible,
                "corners" => target.corners = n.corners,
                "characters" | "textStyle" => {
                    if let (NodeKind::Text(a), NodeKind::Text(b)) = (&n.kind, &mut target.kind) {
                        *b = a.clone();
                    }
                }
                _ => {}
            }
        }
        if let Some(slot) = doc.node_mut(src) {
            *slot = target;
        }
        if let Some(m) = doc.node_mut(d) {
            m.overrides.clear();
        }
    }
    ok()
}

fn set_props(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("instance.setProperties", s, p)?;
    let props = get(p, "properties").and_then(Value::as_object).cloned().ok_or_else(|| bad("instance.setProperties", "`properties` object required"))?;
    let doc = s.doc_mut()?;
    for i in ids {
        let Some(NodeKind::Instance { component, .. }) = doc.node(i).map(|n| n.kind.clone()) else { continue };
        // Variant properties: switch to the matching sibling variant.
        let set = doc.node(component).and_then(|c| c.parent).filter(|pp| doc.node(*pp).is_some_and(|n| matches!(n.kind, NodeKind::ComponentSet { .. })));
        let mut wanted = doc.node(component).map(|c| variant_props(&c.name)).unwrap_or_default();
        let mut variant_change = false;
        for (k, v) in &props {
            if wanted.contains_key(k) {
                wanted.insert(k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
                variant_change = true;
            }
        }
        let n = doc.get_mut(i)?;
        if let NodeKind::Instance { component: c, props: ip } = &mut n.kind {
            for (k, v) in &props {
                ip.insert(k.clone(), v.clone());
            }
            let _ = c;
        }
        if variant_change
            && let Some(set) = set
            && let Some(v) = find_variant(doc, set, &wanted)
            && let NodeKind::Instance { component: c, .. } = &mut doc.get_mut(i)?.kind
        {
            *c = v;
        }
    }
    ok()
}

fn add_prop(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.addProperty";
    let target = targets(s, p).first().copied().ok_or_else(|| bad(cmd, "select a component"))?;
    let name = req_string(cmd, p, "name")?;
    let t = req_string(cmd, p, "type")?;
    let def = match t.to_ascii_lowercase().as_str() {
        "boolean" | "bool" => PropDef::Boolean { default: boolean(p, "default").unwrap_or(true) },
        "text" => PropDef::Text { default: string(p, "default").unwrap_or_default() },
        "instanceswap" | "instance_swap" | "instance" => PropDef::InstanceSwap { default: id(p, "default").ok_or_else(|| bad(cmd, "default: component id"))? },
        "variant" => PropDef::Variant { options: vec![], default: string(p, "default").unwrap_or_default() },
        _ => return Err(bad(cmd, "type: boolean|text|instanceSwap|variant")),
    };
    let layer = id(p, "layer");
    let doc = s.doc_mut()?;
    let n = doc.get_mut(target)?;
    match &mut n.kind {
        NodeKind::Component { props, .. } => {
            props.insert(name.clone(), def.clone());
        }
        NodeKind::ComponentSet { .. } => {
            // Variant property on a set: rename every variant with the new property.
            let kids = n.children.clone();
            let dv = string(p, "default").unwrap_or_else(|| "Default".into());
            for c in kids {
                let cn = doc.get_mut(c)?;
                cn.name = if cn.name.contains('=') { format!("{}, {name}={dv}", cn.name) } else { format!("{name}={dv}") };
            }
            return ok();
        }
        _ => return Err(bad(cmd, "select a component or component set")),
    }
    if let Some(l) = layer {
        let field = match def {
            PropDef::Boolean { .. } => "visible",
            PropDef::Text { .. } => "characters",
            PropDef::InstanceSwap { .. } => "mainComponent",
            PropDef::Variant { .. } => "variant",
        };
        doc.get_mut(l)?.prop_refs.insert(field.into(), name.clone());
    }
    // Existing instances get the default.
    for inst in instances_of(doc, target) {
        if let NodeKind::Instance { props, .. } = &mut doc.get_mut(inst)?.kind {
            let v = match &def {
                PropDef::Boolean { default } => json!(default),
                PropDef::Text { default } => json!(default),
                PropDef::InstanceSwap { default } => json!(default.0),
                PropDef::Variant { default, .. } => json!(default),
            };
            props.entry(name.clone()).or_insert(v);
        }
    }
    ok()
}
