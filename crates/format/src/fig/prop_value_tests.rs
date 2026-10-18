//! Component properties as current `.fig` files write them, on synthetic messages: the copy of a
//! set's definitions each variant keeps (`parentPropDefId`, no name or type of its own).

use figcraft_doc::NodeKind;
use serde_json::Value;

use super::kiwi::Kv;
use super::map::to_rest;

fn g(local: u32) -> Kv<'static> {
    Kv::Object(vec![("sessionID", Kv::Uint(9)), ("localID", Kv::Uint(u64::from(local)))])
}

fn o(fields: Vec<(&'static str, Kv<'static>)>) -> Kv<'static> {
    Kv::Object(fields)
}

fn s(text: &str) -> Kv<'static> {
    Kv::Str(text.into())
}

fn node(local: u32, kind: &'static str, name: &str, parent: Option<u32>, mut fields: Vec<(&'static str, Kv<'static>)>) -> Kv<'static> {
    let mut all = vec![("guid", g(local)), ("type", Kv::Enum(kind)), ("name", s(name))];
    if let Some(parent) = parent {
        all.push(("parentIndex", o(vec![("guid", g(parent)), ("position", s("!"))])));
    }
    all.append(&mut fields);
    o(all)
}

/// A layer's `field` bound to property definition `def`.
fn bound(field: &'static str, def: u32) -> (&'static str, Kv<'static>) {
    let entry = o(vec![
        ("variableField", Kv::Enum(field)),
        ("variableData", o(vec![("value", o(vec![("propRefValue", o(vec![("defId", g(def))]))])), ("dataType", Kv::Enum("PROP_REF"))])),
    ]);
    ("parameterConsumptionMap", o(vec![("entries", Kv::Array(vec![entry]))]))
}

fn definition(id: u32, name: &str, kind: &'static str, initial: Kv<'static>) -> Kv<'static> {
    o(vec![("id", g(id)), ("name", s(name)), ("type", Kv::Enum(kind)), ("initialValue", initial)])
}

fn message(nodes: Vec<Kv<'static>>) -> Kv<'static> {
    let mut all = vec![node(1, "DOCUMENT", "Document", None, vec![]), node(2, "CANVAS", "Page", Some(1), vec![])];
    all.extend(nodes);
    o(vec![("nodeChanges", Kv::Array(all))])
}

fn by_id<'v>(v: &'v Value, id: &str) -> &'v Value {
    fn find<'v>(v: &'v Value, id: &str) -> Option<&'v Value> {
        if v.get("id").and_then(Value::as_str) == Some(id) {
            return Some(v);
        }
        v.get("children").and_then(Value::as_array).into_iter().flatten().find_map(|v| find(v, id))
    }
    find(v, id).unwrap_or_else(|| panic!("no layer {id}"))
}

fn instance_props(doc: &figcraft_doc::Document) -> &std::collections::BTreeMap<String, Value> {
    let instance = doc.ids().filter_map(|id| doc.node(id)).find(|n| n.name == "Instance").unwrap();
    let NodeKind::Instance { props, .. } = &instance.kind else { panic!("not an instance") };
    props
}

#[test]
fn variant_layers_bind_to_the_set_properties_their_copies_name() {
    let msg = message(vec![
        node(
            9,
            "FRAME",
            "Field",
            Some(2),
            vec![
                ("isStateGroup", Kv::Bool(true)),
                ("componentPropDefs", Kv::Array(vec![definition(70, "Hint", "BOOL", o(vec![("boolValue", Kv::Bool(true))]))])),
            ],
        ),
        node(
            10,
            "SYMBOL",
            "State=Default",
            Some(9),
            vec![("componentPropDefs", Kv::Array(vec![o(vec![("id", g(71)), ("parentPropDefId", g(70))])]))],
        ),
        node(11, "TEXT", "Hint text", Some(10), vec![bound("VISIBLE", 71)]),
        node(
            20,
            "INSTANCE",
            "Instance",
            Some(2),
            vec![
                ("symbolData", o(vec![("symbolID", g(10))])),
                ("componentPropAssignments", Kv::Array(vec![o(vec![("defID", g(71)), ("value", o(vec![("boolValue", Kv::Bool(false))]))])])),
            ],
        ),
    ]);
    let (rest, _, _) = to_rest(&msg, "Synthetic", crate::rest::MAX_NODES);
    assert_eq!(by_id(&rest["document"], "9:9")["componentPropertyDefinitions"]["Hint#9:70"]["type"], "BOOLEAN");
    assert_eq!(by_id(&rest["document"], "9:11")["componentPropertyReferences"]["visible"], "Hint#9:70");
    assert_eq!(by_id(&rest["document"], "9:20")["componentProperties"]["Hint#9:70"]["value"], false);

    let (doc, _) = crate::rest::import_fig(&rest, &crate::rest::no_images).unwrap();
    let hint = doc.ids().filter_map(|id| doc.node(id)).find(|n| n.name == "Hint text" && n.source.is_none()).unwrap();
    assert_eq!(hint.prop_refs.get("visible").map(String::as_str), Some("Hint"));
    assert_eq!(instance_props(&doc).get("Hint"), Some(&Value::Bool(false)));
}
