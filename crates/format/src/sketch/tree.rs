//! Sketch pages and layers → REST API file JSON nodes.

use figcraft_doc::VectorNetwork;
use serde_json::{Map as JsonMap, Value, json};

use super::Archive;

/// Deepest layer nesting followed (deeper layers are skipped with a warning).
const MAX_DEPTH: usize = 200;

/// The REST JSON of a Sketch file, with what the mapping noticed.
pub(crate) struct Map {
    pub json: Value,
    pub warnings: Vec<String>,
    /// Vector networks the JSON references by `vectorNetworkRef`.
    pub networks: Vec<Option<VectorNetwork>>,
    /// A text layer stored the pre-48 way was met: the file is refused.
    pub too_old: bool,
}

/// The mapping's state while walking the file.
pub(crate) struct Cx {
    pub warnings: Vec<String>,
    pub networks: Vec<Option<VectorNetwork>>,
    pub too_old: bool,
}

impl Cx {
    pub(crate) fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }
}

/// Map the pages of `archive` to a REST file JSON named `name`.
pub(crate) fn to_rest(archive: &Archive, name: &str) -> Map {
    let mut cx = Cx { warnings: Vec::new(), networks: Vec::new(), too_old: false };
    let mut canvases = Vec::new();
    for (id, page) in &archive.pages {
        let mut node = JsonMap::new();
        node.insert("type".into(), json!("CANVAS"));
        node.insert("id".into(), json!(id));
        node.insert("name".into(), json!(str_field(page, "name").unwrap_or("Page")));
        let kids: Vec<Value> = layers(page).iter().filter_map(|l| layer(&mut cx, l, 0)).collect();
        node.insert("children".into(), Value::Array(kids));
        canvases.push(Value::Object(node));
    }
    let json = json!({"name": name, "document": {"type": "DOCUMENT", "id": "0:0", "children": canvases}});
    Map { json, warnings: cx.warnings, networks: cx.networks, too_old: cx.too_old }
}

/// One Sketch layer as a REST node (with its children), or `None` when it is skipped.
pub(crate) fn layer(cx: &mut Cx, l: &Value, depth: usize) -> Option<Value> {
    if depth > MAX_DEPTH {
        cx.warn(format!("layers nested deeper than {MAX_DEPTH} levels were skipped"));
        return None;
    }
    if is_archived_text(l) {
        cx.too_old = true;
        return None;
    }
    let class = str_field(l, "_class")?;
    cx.warn(format!("Sketch {class} layers are not supported yet and were skipped"));
    None
}

/// A text layer whose string is a binary property list (Sketch 43–47).
fn is_archived_text(l: &Value) -> bool {
    l.get("attributedString").is_some_and(|a| a.get("archivedAttributedString").is_some())
}

/// The child layers of a page or layer.
pub(crate) fn layers(l: &Value) -> &[Value] {
    l.get("layers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

pub(crate) fn str_field<'v>(v: &'v Value, key: &str) -> Option<&'v str> {
    v.get(key).and_then(Value::as_str)
}
