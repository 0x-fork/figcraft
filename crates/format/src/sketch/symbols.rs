//! Sketch symbols → REST components and instances.
//!
//! A `symbolMaster` (on any page, or copied from a library into `document.foreignSymbols`) is a
//! component; a `symbolInstance` names it by `symbolID`. REST instances carry their layers, so an
//! instance is its master's layers with the instance's `overrideValues` applied, laid out at the
//! master's size (the import then resizes them to the instance by their constraints). An override
//! is named `<path>_<property>`: `path` is the `do_objectID` of a layer of the master, or of
//! nested instances and then a layer of theirs, joined by `/`; properties: `stringValue` (text),
//! `symbolID` (swap a nested instance; empty hides it), `image`, `layerStyle` and `textStyle`
//! (shared style ids).

use std::collections::BTreeMap;

use figcraft_doc::geom::Affine;
use serde_json::{Map as JsonMap, Value, json};

use super::tree::{Cx, base, bool_field, children, local, size, str_field};

/// Most masters read from one file.
const MAX_MASTERS: usize = 100_000;

/// Every symbol master of a file by `symbolID`, and the library ones no page holds.
#[derive(Default)]
pub(crate) struct Symbols {
    masters: BTreeMap<String, Value>,
    /// Masters copied from libraries that no page holds (they get a page of their own), with the
    /// libraries' names.
    pub foreign: Vec<Value>,
    pub libraries: Vec<String>,
}

impl Symbols {
    /// The masters of `pages` and of the document's library copies.
    pub(crate) fn read(document: &Value, pages: &[(String, Value)]) -> Symbols {
        let mut s = Symbols::default();
        let mut found = Vec::new();
        for (_, p) in pages {
            masters_in(p, &mut found, 0);
        }
        for m in found {
            s.add(m.clone());
        }
        for f in document.get("foreignSymbols").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
            let Some(m) = f.get("symbolMaster") else { continue };
            let Some(id) = str_field(m, "symbolID") else { continue };
            if s.masters.contains_key(id) {
                continue;
            }
            if let Some(lib) = str_field(f, "sourceLibraryName")
                && !s.libraries.iter().any(|l| l == lib)
            {
                s.libraries.push(lib.chars().take(64).collect());
            }
            s.add(m.clone());
            s.foreign.push(m.clone());
        }
        s
    }

    fn add(&mut self, m: Value) {
        if self.masters.len() >= MAX_MASTERS {
            return;
        }
        if let Some(id) = str_field(&m, "symbolID") {
            self.masters.entry(id.to_string()).or_insert(m);
        }
    }

    fn master(&self, symbol_id: &str) -> Option<&Value> {
        self.masters.get(symbol_id)
    }
}

fn masters_in<'v>(l: &'v Value, out: &mut Vec<&'v Value>, depth: usize) {
    if depth > 64 {
        return;
    }
    for c in super::tree::layers(l) {
        if str_field(c, "_class") == Some("symbolMaster") {
            out.push(c);
        } else {
            masters_in(c, out, depth + 1);
        }
    }
}

/// A master's background as REST fills (masters show one only when they have it on).
pub(crate) fn master_fills(m: &Value) -> Option<Value> {
    if bool_field(m, "hasBackgroundColor") != Some(true) {
        return None;
    }
    let c = m.get("backgroundColor").and_then(super::tree::color)?;
    Some(json!([{"type": "SOLID", "color": c}]))
}

/// A symbol master as a COMPONENT node.
pub(crate) fn component(cx: &mut Cx, l: &Value, parent: Affine, depth: usize, root: bool) -> Option<Value> {
    let (w, h) = size(l);
    let world = parent * local(l, w, h);
    let mut node = base(l, "COMPONENT", parent, world, w, h, root);
    node.insert("clipsContent".into(), json!(true));
    if let Some(f) = master_fills(l) {
        node.insert("fills".into(), f);
    }
    let sid = str_field(l, "symbolID").unwrap_or("").to_string();
    cx.chain.push(sid);
    let kids = children(cx, l, world, (w, h), depth, false);
    cx.chain.pop();
    node.insert("children".into(), Value::Array(kids));
    Some(Value::Object(node))
}

/// A symbol instance as an INSTANCE node holding its master's layers with its overrides, or an
/// empty frame when its master isn't in the file.
pub(crate) fn instance(cx: &mut Cx, l: &Value, parent: Affine, depth: usize, root: bool) -> Option<Value> {
    let (w, h) = size(l);
    let world = parent * local(l, w, h);
    let sid = str_field(l, "symbolID").unwrap_or("");
    let Some(m) = cx.symbols.master(sid).cloned() else {
        cx.warn("instances of symbols that aren't in the file were imported as empty frames");
        let mut node = base(l, "FRAME", parent, world, w, h, root);
        node.insert("children".into(), json!([]));
        return Some(Value::Object(node));
    };
    if cx.chain.iter().any(|c| c == sid) {
        let name: String = str_field(&m, "name").unwrap_or("").chars().take(64).collect();
        cx.warn(format!("symbol {name} contains itself; the inner copy was left out"));
        return None;
    }
    if l.get("scale").and_then(Value::as_f64).is_some_and(|s| s.is_finite() && (s - 1.0).abs() > 1e-9) {
        cx.warn("scaled symbol instances were imported at their size, unscaled");
    }
    let mut node = base(l, "INSTANCE", parent, world, w, h, root);
    node.insert("componentId".into(), json!(str_field(&m, "do_objectID").unwrap_or(sid)));
    node.insert("clipsContent".into(), json!(true));
    if let Some(f) = master_fills(&m) {
        node.insert("fills".into(), f);
    }
    if let Some(st) = l.get("style") {
        super::style::apply(cx, st, w, h, &mut node);
    }
    let overrides = overrides(l);
    let mut copy = m;
    if let Some(layers) = copy.get_mut("layers").and_then(Value::as_array_mut) {
        apply(layers, &overrides, cx.document, 0);
    }
    let (mw, mh) = size(&copy);
    cx.chain.push(sid.to_string());
    let mut kids = children(cx, &copy, world, (mw, mh), depth, false);
    cx.chain.pop();
    let prefix = str_field(l, "do_objectID").unwrap_or("I").to_string();
    for k in &mut kids {
        prefix_ids(k, &prefix, 0);
    }
    node.insert("children".into(), Value::Array(kids));
    Some(Value::Object(node))
}

/// An instance's overrides by name (`<path>_<property>` → value).
fn overrides(l: &Value) -> BTreeMap<String, Value> {
    l.get("overrideValues")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .take(100_000)
        .filter_map(|o| Some((str_field(o, "overrideName")?.to_string(), o.get("value")?.clone())))
        .collect()
}

/// Apply `ov` to the layers of a master's copy: to the layers it names, and passed on to nested
/// instances for the layers inside them (an outer override wins over the nested instance's own).
fn apply(layers: &mut [Value], ov: &BTreeMap<String, Value>, document: &Value, depth: usize) {
    if depth > 64 || ov.is_empty() {
        return;
    }
    for l in layers {
        let Some(id) = str_field(l, "do_objectID").map(str::to_string) else { continue };
        let get = |prop: &str| ov.get(&format!("{id}_{prop}"));
        match str_field(l, "_class") {
            Some("symbolInstance") => {
                match get("symbolID").map(|v| v.as_str().unwrap_or("")) {
                    Some("") => l["isVisible"] = json!(false),
                    Some(other) => l["symbolID"] = json!(other),
                    None => {}
                }
                let prefix = format!("{id}/");
                let inner: Vec<Value> = ov
                    .iter()
                    .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|rest| json!({"_class": "overrideValue", "overrideName": rest, "value": v})))
                    .collect();
                if !inner.is_empty() {
                    let mut own = l.get("overrideValues").and_then(Value::as_array).cloned().unwrap_or_default();
                    own.extend(inner);
                    l["overrideValues"] = Value::Array(own);
                }
                continue;
            }
            Some("text") => {
                if let Some(s) = get("stringValue").and_then(Value::as_str) {
                    set_string(l, s);
                }
            }
            Some("bitmap") => {
                if let Some(img) = get("image").filter(|v| v.get("_ref").is_some()) {
                    l["image"] = img.clone();
                }
            }
            _ => {}
        }
        for (prop, list) in [("layerStyle", "layerStyles"), ("textStyle", "layerTextStyles")] {
            if let Some(style) = get(prop).and_then(Value::as_str).and_then(|sid| shared_style(document, list, sid)) {
                l["style"] = style.clone();
                if prop == "textStyle"
                    && let Some(enc) = style.get("textStyle").and_then(|t| t.get("encodedAttributes"))
                    && let Some(runs) = l.get_mut("attributedString").and_then(|a| a.get_mut("attributes")).and_then(Value::as_array_mut)
                {
                    for r in runs {
                        r["attributes"] = enc.clone();
                    }
                }
            }
        }
        if let Some(kids) = l.get_mut("layers").and_then(Value::as_array_mut) {
            apply(kids, ov, document, depth + 1);
        }
    }
}

/// A text layer's string replaced, in the style of its first run.
fn set_string(l: &mut Value, s: &str) {
    let units = s.encode_utf16().count();
    let first = l.get("attributedString").and_then(|a| a.get("attributes")).and_then(Value::as_array).and_then(|a| a.first()).cloned();
    let Some(a) = l.get_mut("attributedString").and_then(Value::as_object_mut) else { return };
    a.insert("string".into(), json!(s));
    let runs = match first {
        Some(mut r) => {
            r["location"] = json!(0);
            r["length"] = json!(units);
            vec![r]
        }
        None => Vec::new(),
    };
    a.insert("attributes".into(), Value::Array(runs));
}

/// The style of shared style `id` from the document's `list` (local) or its library copies.
pub(crate) fn shared_style<'v>(document: &'v Value, list: &str, id: &str) -> Option<&'v Value> {
    let local = document.get(list).and_then(|c| c.get("objects")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let foreign_key = if list == "layerStyles" { "foreignLayerStyles" } else { "foreignTextStyles" };
    let foreign = document.get(foreign_key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    local
        .iter()
        .find(|s| str_field(s, "do_objectID") == Some(id))
        .or_else(|| foreign.iter().filter_map(|f| f.get("localSharedStyle")).find(|s| str_field(s, "do_objectID") == Some(id)))
        .and_then(|s| s.get("value"))
}

/// Make the ids of an instance's layers its own (`<instance>;<layer>`): the master's layers keep
/// theirs.
fn prefix_ids(v: &mut Value, prefix: &str, depth: usize) {
    if depth > 256 {
        return;
    }
    let Some(m) = v.as_object_mut() else { return };
    if let Some(id) = m.get("id").and_then(Value::as_str).map(str::to_string) {
        m.insert("id".into(), json!(format!("{prefix};{id}")));
    }
    if let Some(kids) = m.get_mut("children").and_then(Value::as_array_mut) {
        for k in kids {
            prefix_ids(k, prefix, depth + 1);
        }
    }
}

/// Library masters no page holds: a page of their own, side by side.
pub(crate) fn library_page(cx: &mut Cx, masters: &[Value]) -> Value {
    let mut x = 0.0;
    let mut kids = Vec::new();
    for m in masters {
        let mut placed = m.clone();
        let (w, _) = size(m);
        if let Some(f) = placed.get_mut("frame").and_then(Value::as_object_mut) {
            f.insert("x".into(), json!(x));
            f.insert("y".into(), json!(0.0));
        }
        x += w + 100.0;
        if let Some(node) = component(cx, &placed, Affine::IDENTITY, 0, true) {
            kids.push(node);
        }
    }
    let mut page = JsonMap::new();
    page.insert("type".into(), json!("CANVAS"));
    page.insert("id".into(), json!("library-symbols"));
    page.insert("name".into(), json!("Library symbols"));
    page.insert("children".into(), Value::Array(kids));
    Value::Object(page)
}
