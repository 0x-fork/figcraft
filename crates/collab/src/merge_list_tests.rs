//! The layer lists whose items stand on their own (fills, strokes, effects, export settings,
//! guides, prototype interactions, accessibility annotations, Dev Mode links and resources) merge
//! item by item: different items changed on each side merge, the same item changed both ways
//! conflicts.

use figcraft_color::{Color, Paint};
use figcraft_doc::{Document, Effect, NodeId, NodeKind};
use serde_json::{Value, json};

use crate::merge::{Key, Side, merge_items, plan};

/// A file with one rectangle (two fills, one stroke, two effects), a branch of it and main.
fn files() -> (Document, Document, Document, NodeId) {
    let mut base = Document::new("File");
    let page = base.first_page().unwrap();
    let r = base.create(page, NodeKind::Rectangle, "R", 0.0, 0.0, 10.0, 10.0).unwrap();
    let n = base.get_mut(r).unwrap();
    n.fills = vec![Paint::solid(Color::BLACK), Paint::solid(Color::WHITE)];
    n.strokes = vec![Paint::solid(Color::BLACK)];
    n.effects = vec![Effect::drop_shadow(), Effect::layer_blur()];
    let mut branch = base.clone();
    branch.set_client_id(base.highest_id_space() + 1);
    let main = base.clone();
    (base, branch, main, r)
}

fn red() -> Paint {
    Paint::solid(Color::from_rgb8_const(255, 0, 0))
}

fn blue() -> Paint {
    Paint::solid(Color::from_rgb8_const(0, 0, 255))
}

/// Merging either way gives `check` the same result.
fn both_ways(base: &Document, branch: &Document, main: &Document, check: impl Fn(&Document)) {
    let p = plan(base, branch, main).unwrap();
    assert!(p.conflicts.is_empty(), "{:?}", p.conflicts);
    for (into, from) in [(main, Side::Branch), (branch, Side::Main)] {
        let (doc, _) = p.merge(into, from, &|_| None).unwrap();
        assert!(doc.validate().is_empty());
        check(&doc);
    }
}

#[test]
fn different_items_of_a_list_merge() {
    // The branch recolours the first fill and hides the shadow; main recolours the second fill
    // and hides the blur.
    let (base, mut branch, mut main, r) = files();
    branch.get_mut(r).unwrap().fills[0] = red();
    branch.get_mut(r).unwrap().effects[0].visible = false;
    main.get_mut(r).unwrap().fills[1] = blue();
    main.get_mut(r).unwrap().effects[1].visible = false;
    both_ways(&base, &branch, &main, |doc| {
        let n = doc.node(r).unwrap();
        assert_eq!(n.fills, vec![red(), blue()]);
        assert!(!n.effects[0].visible && !n.effects[1].visible);
    });
}

#[test]
fn items_added_on_one_side_are_kept() {
    // The branch adds a stroke; main recolours the existing one. The same item added on both
    // sides is kept once.
    let (base, mut branch, mut main, r) = files();
    branch.get_mut(r).unwrap().strokes.push(red());
    main.get_mut(r).unwrap().strokes[0] = blue();
    branch.get_mut(r).unwrap().fills.push(red());
    main.get_mut(r).unwrap().fills.push(red());
    both_ways(&base, &branch, &main, |doc| {
        let n = doc.node(r).unwrap();
        assert_eq!(n.strokes, vec![blue(), red()]);
        assert_eq!(n.fills.len(), 3);
    });
}

#[test]
fn the_same_item_changed_both_ways_conflicts() {
    let (base, mut branch, mut main, r) = files();
    branch.get_mut(r).unwrap().fills[0] = red();
    main.get_mut(r).unwrap().fills[0] = blue();
    // A removed item on one side while the other changed the list conflicts too (items have no
    // ids to line them up by).
    branch.get_mut(r).unwrap().effects.remove(0);
    main.get_mut(r).unwrap().effects[1].visible = false;
    // Different items added at the end on both sides conflict.
    branch.get_mut(r).unwrap().strokes.push(red());
    main.get_mut(r).unwrap().strokes.push(blue());
    let p = plan(&base, &branch, &main).unwrap();
    let mut keys: Vec<String> = p.conflicts.iter().map(|c| c.key.to_string()).collect();
    keys.sort();
    let mut want = [format!("{r}/effects"), format!("{r}/fills"), format!("{r}/strokes")];
    want.sort();
    assert_eq!(keys, want);
    // Resolving takes that side's whole list.
    let pick = |s: Side| move |_: &Key| Some(s);
    let (doc, _) = p.merge(&main, Side::Branch, &pick(Side::Branch)).unwrap();
    assert_eq!(doc.node(r).unwrap().fills, branch.node(r).unwrap().fills);
    let (doc, _) = p.merge(&main, Side::Branch, &pick(Side::Main)).unwrap();
    assert_eq!(doc.node(r).unwrap().fills, main.node(r).unwrap().fills);
}

#[test]
fn the_other_item_lists_merge_one_by_one_too() {
    // Export settings, guides, prototype interactions, accessibility annotations and the Dev Mode
    // links and resources are item lists like fills: both sides changing different items merges.
    let (mut base, _, _, r) = files();
    let export = |suffix: &str| figcraft_doc::ExportSetting { suffix: suffix.into(), ..Default::default() };
    let guide = |vertical: bool, offset: f64| figcraft_doc::Guide { vertical, offset };
    let annot = |label: &str| figcraft_doc::Annotation { label: label.into(), properties: Vec::new(), category: None };
    let link =
        |name: &str| figcraft_doc::CodeLink { language: "react".into(), import_path: String::new(), name: name.into(), template: String::new() };
    let res = |url: &str| figcraft_doc::DevResource { url: url.into(), name: String::new() };
    let n = base.get_mut(r).unwrap();
    n.exports = vec![export("@1"), export("@2")];
    n.guides = vec![guide(true, 10.0), guide(false, 20.0)];
    n.annotations = vec![annot("a1"), annot("a2")];
    n.code_links = vec![link("L1"), link("L2")];
    n.dev_resources = vec![res("https://a.test"), res("https://b.test")];
    let (mut branch, mut main) = (base.clone(), base.clone());
    branch.set_client_id(base.highest_id_space() + 1);
    // The branch changes the first item of each list; main the second.
    let b = branch.get_mut(r).unwrap();
    (b.exports[0].suffix, b.guides[0].offset, b.annotations[0].label, b.code_links[0].name, b.dev_resources[0].url) =
        ("@b".into(), 11.0, "b1".into(), "LB".into(), "https://branch.test".into());
    let m = main.get_mut(r).unwrap();
    (m.exports[1].suffix, m.guides[1].offset, m.annotations[1].label, m.code_links[1].name, m.dev_resources[1].url) =
        ("@m".into(), 22.0, "m2".into(), "LM".into(), "https://main.test".into());
    both_ways(&base, &branch, &main, |doc| {
        let n = doc.node(r).unwrap();
        let suffixes: Vec<&str> = n.exports.iter().map(|e| e.suffix.as_str()).collect();
        assert_eq!(suffixes, ["@b", "@m"], "export settings");
        let offsets: Vec<f64> = n.guides.iter().map(|g| g.offset).collect();
        assert_eq!(offsets, [11.0, 22.0], "guides");
        let labels: Vec<&str> = n.annotations.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, ["b1", "m2"], "annotations");
        let names: Vec<&str> = n.code_links.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["LB", "LM"], "code links");
        let urls: Vec<&str> = n.dev_resources.iter().map(|d| d.url.as_str()).collect();
        assert_eq!(urls, ["https://branch.test", "https://main.test"], "dev resources");
    });
}

#[test]
fn the_same_item_of_another_list_changed_both_ways_conflicts() {
    let (mut base, _, _, r) = files();
    base.get_mut(r).unwrap().exports = vec![
        figcraft_doc::ExportSetting { suffix: "@1".into(), ..Default::default() },
        figcraft_doc::ExportSetting { suffix: "@2".into(), ..Default::default() },
    ];
    let (mut branch, mut main) = (base.clone(), base.clone());
    branch.set_client_id(base.highest_id_space() + 1);
    branch.get_mut(r).unwrap().exports[0].suffix = "@b".into();
    main.get_mut(r).unwrap().exports[0].suffix = "@m".into();
    let p = plan(&base, &branch, &main).unwrap();
    let keys: Vec<String> = p.conflicts.iter().map(|c| c.key.to_string()).collect();
    assert_eq!(keys, [format!("{r}/exports")], "the same item changed both ways still conflicts");
}

#[test]
fn item_merge_rules() {
    let v = |x: Value| x.as_array().unwrap().clone();
    let (o, b, m) = (v(json!([1, 2, 3])), v(json!([9, 2, 3])), v(json!([1, 2, 8])));
    assert_eq!(merge_items(&o, &b, &m), Some(v(json!([9, 2, 8]))));
    assert_eq!(merge_items(&o, &b, &b), Some(b.clone()), "the same change on both sides");
    assert_eq!(merge_items(&o, &b, &v(json!([7, 2, 3]))), None, "the same item both ways");
    assert_eq!(merge_items(&o, &v(json!([1, 2])), &m), None, "an item removed");
    assert_eq!(merge_items(&o, &v(json!([1, 2, 3, 4])), &m), Some(v(json!([1, 2, 8, 4]))));
    assert_eq!(merge_items(&o, &v(json!([1, 2, 3, 4])), &v(json!([1, 2, 3, 5]))), None);
    assert_eq!(merge_items(&[], &v(json!([1])), &[]), Some(v(json!([1]))));
}
