//! Components and instances.
//!
//! An instance's layers are real nodes copied from its main component; each copy records the
//! main-component node it mirrors (`source`) and which property groups were changed locally
//! (`overrides`). When a main component changes, every instance is rebuilt from it, keeping layer
//! ids and re-applying overrides. Variants are components inside a component set, named
//! `Property=Value, …`; component properties (boolean, text, instance swap, variant) are applied
//! through `prop_refs` on the layers they control.

use std::collections::{BTreeMap, BTreeSet};

use figcraft_doc::*;
use figcraft_geom::Affine;
use serde_json::Value;

/// The component a node belongs to (itself, or the nearest component ancestor).
pub fn owning_component(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut chain = vec![id];
    chain.extend(doc.ancestors(id));
    chain.into_iter().find(|c| doc.node(*c).is_some_and(|n| matches!(n.kind, NodeKind::Component { .. })))
}

/// The instance root containing `id` (excluding `id` itself unless it is an instance).
pub fn owning_instance(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut chain = vec![id];
    chain.extend(doc.ancestors(id));
    chain.into_iter().find(|c| doc.node(*c).is_some_and(|n| matches!(n.kind, NodeKind::Instance { .. })))
}

/// All instances of `component` in the document.
pub fn instances_of(doc: &Document, component: NodeId) -> Vec<NodeId> {
    doc.nodes().filter(|n| matches!(n.kind, NodeKind::Instance { component: c, .. } if c == component)).map(|n| n.id).collect()
}

/// Create an instance of `component` under `parent` at `(x, y)` (parent space).
pub fn create_instance(doc: &mut Document, component: NodeId, parent: NodeId, x: f64, y: f64) -> Result<NodeId, DocError> {
    let comp = doc.get(component)?.clone();
    if !matches!(comp.kind, NodeKind::Component { .. }) {
        return Err(DocError::Invalid(format!("{component} is not a component")));
    }
    let id = doc.alloc_id();
    let mut root = comp.clone();
    root.id = id;
    root.kind = NodeKind::Instance { component, props: default_props(&comp) };
    root.children.clear();
    root.parent = None;
    root.source = Some(component);
    root.overrides.clear();
    root.transform = Affine::translate((x, y));
    root.prop_refs.clear();
    doc.insert(root, parent, None)?;
    sync_instance(doc, id);
    Ok(id)
}

fn default_props(comp: &Node) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    if let NodeKind::Component { props, .. } = &comp.kind {
        for (k, d) in props {
            let v = match d {
                PropDef::Boolean { default } => Value::Bool(*default),
                PropDef::Text { default } => Value::String(default.clone()),
                PropDef::InstanceSwap { default } => Value::from(default.0),
                PropDef::Variant { default, .. } => Value::String(default.clone()),
            };
            out.insert(k.clone(), v);
        }
    }
    out
}

/// Copy the overridden property groups of `from` onto `to`.
fn carry_overrides(from: &Node, to: &mut Node) {
    for g in &from.overrides {
        match g.as_str() {
            "fills" => to.fills = from.fills.clone(),
            "strokes" => {
                to.strokes = from.strokes.clone();
                to.stroke = from.stroke.clone();
            }
            "effects" => to.effects = from.effects.clone(),
            "opacity" => {
                to.opacity = from.opacity;
                to.blend = from.blend;
            }
            "visible" => to.visible = from.visible,
            "name" => to.name = from.name.clone(),
            "corners" => to.corners = from.corners,
            "characters" => {
                if let (NodeKind::Text(a), NodeKind::Text(b)) = (&from.kind, &mut to.kind) {
                    b.characters = a.characters.clone();
                    b.runs = a.runs.clone();
                }
            }
            "textStyle" => {
                if let (NodeKind::Text(a), NodeKind::Text(b)) = (&from.kind, &mut to.kind) {
                    b.style = a.style.clone();
                    b.runs = a.runs.clone();
                    b.align_h = a.align_h;
                    b.align_v = a.align_v;
                }
            }
            "size" => {
                to.width = from.width;
                to.height = from.height;
                to.child_layout = from.child_layout.clone();
            }
            "component" => {
                if let (NodeKind::Instance { component: a, props: pa }, NodeKind::Instance { component: b, props: pb }) = (&from.kind, &mut to.kind) {
                    *b = *a;
                    *pb = pa.clone();
                }
            }
            _ => {}
        }
    }
    to.overrides = from.overrides.clone();
}

/// Name path from the instance root (for matching layers across variants).
fn name_path(doc: &Document, root: NodeId, id: NodeId) -> String {
    let mut parts = Vec::new();
    let mut cur = Some(id);
    while let Some(c) = cur {
        if c == root {
            break;
        }
        let Some(n) = doc.node(c) else { break };
        parts.push(n.name.clone());
        cur = n.parent;
        if parts.len() > 256 {
            break;
        }
    }
    parts.reverse();
    parts.join("/")
}

/// Rebuild instance `inst` from its main component, keeping ids and overrides.
pub fn sync_instance(doc: &mut Document, inst: NodeId) {
    sync_instance_depth(doc, inst, 0);
}

fn sync_instance_depth(doc: &mut Document, inst: NodeId, depth: usize) {
    if depth > 16 {
        return;
    }
    let Some(root) = doc.node(inst).cloned() else { return };
    let NodeKind::Instance { component, props } = &root.kind else { return };
    let (component, props) = (*component, props.clone());
    let Some(comp) = doc.node(component).cloned() else { return };
    if doc.is_ancestor(inst, component) {
        // An instance inside its own main component would recurse forever.
        return;
    }
    // Old layers by source and by name path.
    let old_ids: Vec<NodeId> = doc.subtree(inst).into_iter().filter(|i| *i != inst).collect();
    let mut by_source: BTreeMap<NodeId, Node> = BTreeMap::new();
    let mut by_path: BTreeMap<String, Node> = BTreeMap::new();
    for i in &old_ids {
        if let Some(n) = doc.node(*i) {
            if let Some(s) = n.source {
                by_source.insert(s, n.clone());
            }
            by_path.insert(name_path(doc, inst, *i), n.clone());
        }
    }
    // Remove the old layers.
    let kids = doc.node(inst).map(|n| n.children.clone()).unwrap_or_default();
    for c in kids {
        let _ = doc.remove(c);
    }
    // Copy the component's layers.
    let src: Vec<NodeId> = doc.subtree(component).into_iter().filter(|i| *i != component).collect();
    let mut map: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    let mut used: BTreeSet<NodeId> = BTreeSet::new();
    for s in &src {
        let path = name_path(doc, component, *s);
        let old = by_source.get(s).or_else(|| by_path.get(&path));
        let id = match old {
            Some(o) if !doc.contains(o.id) && used.insert(o.id) => o.id,
            _ => doc.alloc_id(),
        };
        map.insert(*s, id);
    }
    map.insert(component, inst);
    let mut new_nodes = Vec::new();
    for s in &src {
        let Some(orig) = doc.node(*s).cloned() else { continue };
        let Some(&nid) = map.get(s) else { continue };
        let mut n = orig.clone();
        n.id = nid;
        n.parent = orig.parent.and_then(|p| map.get(&p).copied());
        n.children = orig.children.iter().filter_map(|c| map.get(c).copied()).collect();
        // Nested instance copies keep their own main; layers mirror the main's layer.
        n.source = Some(orig.source.filter(|_| false).unwrap_or(*s));
        n.overrides.clear();
        let path = name_path(doc, component, *s);
        if let Some(old) = by_source.get(s).or_else(|| by_path.get(&path)) {
            carry_overrides(old, &mut n);
        }
        // Component properties.
        for (field, prop) in &orig.prop_refs {
            let Some(val) = props.get(prop) else { continue };
            match field.as_str() {
                "visible" => {
                    if let Some(b) = val.as_bool() {
                        n.visible = b;
                    }
                }
                "characters" => {
                    if let (NodeKind::Text(t), Some(sv)) = (&mut n.kind, val.as_str())
                        && !n.overrides.contains("characters")
                    {
                        let len = t.char_count();
                        t.replace_range(0, len, sv);
                    }
                }
                "mainComponent" => {
                    if let (NodeKind::Instance { component: c, .. }, Some(v)) = (&mut n.kind, crate::params::id_of(val)) {
                        *c = v;
                    }
                }
                _ => {}
            }
        }
        new_nodes.push(n);
    }
    for n in new_nodes {
        let id = n.id;
        let parent = n.parent;
        let _ = doc_insert_raw(doc, n);
        if parent == Some(inst)
            && let Some(r) = doc.node_mut(inst)
            && !r.children.contains(&id)
        {
            r.children.push(id);
        }
    }
    // Keep the main's child order.
    let order: Vec<NodeId> = comp.children.iter().filter_map(|c| map.get(c).copied()).collect();
    if let Some(r) = doc.node_mut(inst) {
        r.children = order;
    }
    // Root properties follow the main unless overridden.
    if let Some(r) = doc.node_mut(inst) {
        let mut fresh = comp.clone();
        fresh.id = inst;
        fresh.parent = r.parent;
        fresh.children = r.children.clone();
        fresh.transform = r.transform;
        fresh.kind = r.kind.clone();
        fresh.source = Some(component);
        fresh.constraints = r.constraints;
        fresh.child_layout = r.child_layout.clone();
        fresh.interactions = r.interactions.clone();
        fresh.exports = r.exports.clone();
        fresh.prop_refs.clear();
        fresh.locked = r.locked;
        fresh.visible = r.visible;
        fresh.name = r.name.clone();
        fresh.var_modes = r.var_modes.clone();
        // Instances keep their own size (resizing an instance is allowed).
        fresh.width = r.width;
        fresh.height = r.height;
        carry_overrides(&r.clone(), &mut fresh);
        *r = fresh;
    }
    // Instance size differs from main: children follow constraints.
    let (cw, ch) = (comp.width, comp.height);
    let (iw, ih) = doc.node(inst).map(|n| (n.width, n.height)).unwrap_or((cw, ch));
    if ((iw - cw).abs() > 1e-9 || (ih - ch).abs() > 1e-9) && doc.node(inst).is_some_and(|n| n.layout.is_none()) {
        figcraft_layout::apply_constraints(doc, inst, cw, ch);
    }
    // Nested instances inside the copy.
    let nested: Vec<NodeId> =
        doc.subtree(inst).into_iter().filter(|i| *i != inst && doc.node(*i).is_some_and(|n| matches!(n.kind, NodeKind::Instance { .. }))).collect();
    for n in nested {
        // Only re-sync nested instances whose main was swapped by a property or override.
        let needs = doc.node(n).is_some_and(|nn| nn.overrides.contains("component") || !nn.prop_refs.is_empty());
        if needs {
            sync_instance_depth(doc, n, depth + 1);
        }
    }
}

/// Insert a node whose parent/children links are already set (used when rebuilding instances).
fn doc_insert_raw(doc: &mut Document, n: Node) -> Result<(), DocError> {
    let parent = n.parent.ok_or_else(|| DocError::Invalid("no parent".into()))?;
    let id = n.id;
    let mut n = n;
    let kids = std::mem::take(&mut n.children);
    doc.insert(n, parent, None)?;
    // `insert` appended us to the parent; fix sibling order later. Restore children list.
    if let Some(m) = doc.node_mut(id) {
        m.children = kids;
    }
    // Parent may have listed us already (inserted twice): dedupe.
    if let Some(p) = doc.node_mut(parent) {
        let mut seen = BTreeSet::new();
        p.children.retain(|c| seen.insert(*c));
    }
    Ok(())
}

/// Property groups that differ between two versions of a node.
fn diff_groups(a: &Node, b: &Node) -> Vec<&'static str> {
    let mut out = Vec::new();
    if a.fills != b.fills {
        out.push("fills");
    }
    if a.strokes != b.strokes || a.stroke != b.stroke {
        out.push("strokes");
    }
    if a.effects != b.effects {
        out.push("effects");
    }
    if a.opacity != b.opacity || a.blend != b.blend {
        out.push("opacity");
    }
    if a.visible != b.visible {
        out.push("visible");
    }
    if a.name != b.name {
        out.push("name");
    }
    if a.corners != b.corners {
        out.push("corners");
    }
    if let (NodeKind::Text(x), NodeKind::Text(y)) = (&a.kind, &b.kind) {
        if x.characters != y.characters {
            out.push("characters");
        }
        if x.style != y.style || x.runs != y.runs {
            out.push("textStyle");
        }
    }
    if let (NodeKind::Instance { component: x, .. }, NodeKind::Instance { component: y, .. }) = (&a.kind, &b.kind)
        && x != y
    {
        out.push("component");
    }
    out
}

/// After an edit: record overrides inside instances, then rebuild instances of edited components.
pub fn sync_after_edit(doc: &mut Document, before: &Document, changed: &[NodeId]) {
    // 1. Overrides for layers edited inside instances.
    for id in changed {
        let Some(n) = doc.node(*id) else { continue };
        if n.source.is_none()
            || matches!(n.kind, NodeKind::Instance { .. })
                && owning_instance(doc, *id) == Some(*id)
                && n.parent.is_some_and(|p| owning_instance(doc, p).is_none())
        {
            // Root instances: switching the main is not an override; other edits are.
            if let (Some(b), Some(n)) = (before.node(*id), doc.node(*id))
                && matches!(n.kind, NodeKind::Instance { .. })
            {
                let groups: Vec<&str> = diff_groups(b, n).into_iter().filter(|g| *g != "component").collect();
                if !groups.is_empty()
                    && let Some(m) = doc.node_mut(*id)
                {
                    for g in groups {
                        m.overrides.insert(g.to_string());
                    }
                }
            }
            continue;
        }
        if let Some(b) = before.node(*id) {
            let groups = diff_groups(b, n);
            if !groups.is_empty()
                && let Some(m) = doc.node_mut(*id)
            {
                for g in groups {
                    m.overrides.insert(g.to_string());
                }
            }
        }
    }
    // 2. Components whose layers changed (or lost layers).
    let mut comps: BTreeSet<NodeId> = BTreeSet::new();
    for id in changed {
        // Edits inside an instance aren't edits of the main.
        if let Some(c) = owning_component(doc, *id)
            && owning_instance(doc, *id).is_none_or(|i| !doc.is_ancestor(c, i))
        {
            comps.insert(c);
        }
    }
    for id in before.ids() {
        if !doc.contains(id)
            && let Some(c) = owning_component(before, id)
            && doc.contains(c)
        {
            comps.insert(c);
        }
    }
    // Instance main swapped or props changed: re-sync those instances.
    let mut direct: BTreeSet<NodeId> = BTreeSet::new();
    for id in changed {
        if let (Some(n), Some(b)) = (doc.node(*id), before.node(*id))
            && let (NodeKind::Instance { component: c1, props: p1 }, NodeKind::Instance { component: c2, props: p2 }) = (&n.kind, &b.kind)
            && (c1 != c2 || p1 != p2)
        {
            direct.insert(*id);
        }
    }
    let mut done: BTreeSet<NodeId> = BTreeSet::new();
    let mut frontier: Vec<NodeId> = comps.into_iter().collect();
    let mut rounds = 0;
    while !frontier.is_empty() && rounds < 8 {
        rounds += 1;
        let mut next = Vec::new();
        for c in frontier {
            if !done.insert(c) {
                continue;
            }
            for inst in instances_of(doc, c) {
                sync_instance(doc, inst);
                // The instance may live inside another component: that one changed too.
                if let Some(outer) = owning_component(doc, inst)
                    && !done.contains(&outer)
                {
                    next.push(outer);
                }
            }
        }
        frontier = next;
    }
    for i in direct {
        if doc.contains(i) {
            sync_instance(doc, i);
        }
    }
}

/// Parse a variant name "Size=Large, State=Hover" into pairs.
pub fn variant_props(name: &str) -> BTreeMap<String, String> {
    name.split(',')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// The variant in `set` matching `wanted` best (all given properties equal).
pub fn find_variant(doc: &Document, set: NodeId, wanted: &BTreeMap<String, String>) -> Option<NodeId> {
    let s = doc.node(set)?;
    s.children.iter().copied().find(|c| {
        doc.node(*c).is_some_and(|n| {
            let p = variant_props(&n.name);
            wanted.iter().all(|(k, v)| p.get(k) == Some(v))
        })
    })
}
