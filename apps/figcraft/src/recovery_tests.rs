//! Recovery copies: a run that ends without closing leaves copies of its unsaved documents that the
//! next start opens again (a running app's are left alone); a normal quit leaves nothing.

use figcraft_engine::Session;
use serde_json::json;

use crate::recovery::{Recovery, recover};

fn root(name: &str) -> std::path::PathBuf {
    let r = std::env::temp_dir().join(format!("figcraft-recovery-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&r);
    r
}

fn edited() -> Session {
    let mut s = Session::new();
    s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
    s
}

#[test]
fn a_crashed_runs_unsaved_documents_come_back() {
    let root = root("crash");
    let s = edited();
    let mut r = Recovery::start(&root).unwrap();
    r.write(&s, true);
    // While it runs, another start leaves its copies alone.
    let mut other = Session::empty();
    assert_eq!(recover(&root, &mut other), 0);
    // It ends without closing (the lock goes with the process).
    drop(r);
    let mut next = Session::empty();
    assert_eq!(recover(&root, &mut next), 1);
    let ds = next.docs.first().unwrap();
    assert!(ds.is_dirty() && ds.path.is_none());
    assert_eq!(next.doc().unwrap().node(next.doc().unwrap().pages[0]).map(|p| p.children.len()), Some(1));
    assert!(std::fs::read_dir(&root).unwrap().next().is_none(), "the folder is gone");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn copies_follow_saves_and_a_normal_quit_leaves_nothing() {
    let root = root("quit");
    let mut s = edited();
    let mut r = Recovery::start(&root).unwrap();
    r.write(&s, true);
    let dir = std::fs::read_dir(&root).unwrap().flatten().next().unwrap().path();
    let copies =
        |d: &std::path::Path| std::fs::read_dir(d).unwrap().flatten().filter(|f| f.path().extension().is_some_and(|e| e == "figcraft")).count();
    assert_eq!(copies(&dir), 1);
    // Saved: the copy goes.
    s.ds_mut().unwrap().mark_saved();
    r.write(&s, true);
    assert_eq!(copies(&dir), 0);
    // Changes pending while the interval runs: it asks to be woken for them.
    s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 4, "height": 4})).unwrap();
    assert!(r.tick(&s, 0.0).is_none());
    assert!(r.tick(&s, 1.0).is_none(), "copied at 0 s: nothing new");
    s.execute("shape.rectangle", &json!({"x": 9, "y": 0, "width": 4, "height": 4})).unwrap();
    assert!(r.tick(&s, 1.0).is_some_and(|w| w > 0.0));
    r.finish();
    assert!(!dir.exists(), "a normal quit removes the copies");
    let _ = std::fs::remove_dir_all(&root);
}

/// The rectangles in `doc`.
fn rectangles(doc: &figcraft_engine::doc::Document) -> usize {
    doc.nodes().filter(|n| matches!(n.kind, figcraft_engine::doc::NodeKind::Rectangle)).count()
}

/// The documents a crashed run's copies under `root` open as.
fn crash_and_recover(r: Recovery, root: &std::path::Path) -> Session {
    drop(r);
    let mut next = Session::empty();
    assert_eq!(recover(root, &mut next), 1);
    next
}

#[test]
fn copies_keep_the_version_history_and_branches() {
    // #185: a copy has what a save would write, the latest edits on a branch tab included.
    let root = root("history");
    let mut s = edited();
    s.execute("file.saveVersion", &json!({"name": "v1"})).unwrap();
    s.execute("branch.create", &json!({"name": "Explore"})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 20, "y": 0, "width": 10, "height": 10})).unwrap();
    let mut r = Recovery::start(&root).unwrap();
    r.write(&s, true);
    let next = crash_and_recover(r, &root);
    let ds = next.docs.first().unwrap();
    assert_eq!(ds.versions.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["v1"]);
    assert_eq!(ds.branches.iter().map(|b| (b.name.as_str(), rectangles(&b.head))).collect::<Vec<_>>(), [("Explore", 2)]);
    let _ = std::fs::remove_dir_all(&root);
}

/// The one copy under `root` (the run's folder), read back.
fn copy(root: &std::path::Path) -> figcraft_format::Loaded {
    let dir = std::fs::read_dir(root).unwrap().flatten().next().unwrap().path();
    let file = std::fs::read_dir(dir).unwrap().flatten().map(|f| f.path()).find(|f| f.extension().is_some_and(|e| e == "figcraft")).unwrap();
    figcraft_format::load_bytes_with_versions(&std::fs::read(file).unwrap()).unwrap()
}

#[test]
fn a_change_only_on_a_branch_tab_or_to_the_history_is_copied() {
    let root = root("history-only");
    let mut s = edited();
    s.execute("file.saveVersion", &json!({"name": "v1"})).unwrap();
    s.execute("branch.create", &json!({"name": "Explore"})).unwrap();
    for d in &mut s.docs {
        d.mark_saved();
    }
    let mut r = Recovery::start(&root).unwrap();
    // An edit on the branch tab only: the main file has no changes of its own yet.
    s.execute("shape.rectangle", &json!({"x": 20, "y": 0, "width": 10, "height": 10})).unwrap();
    r.write(&s, true);
    assert_eq!(copy(&root).branches.iter().map(|b| rectangles(&b.head)).collect::<Vec<_>>(), [2]);
    s.execute("shape.rectangle", &json!({"x": 40, "y": 0, "width": 10, "height": 10})).unwrap();
    r.write(&s, true);
    assert_eq!(copy(&root).branches.iter().map(|b| rectangles(&b.head)).collect::<Vec<_>>(), [3]);
    // A version renamed on the main file leaves its document as it was.
    s.execute("doc.switch", &json!({"index": 0})).unwrap();
    r.write(&s, true);
    let id = s.docs[0].versions[0].id.clone();
    s.execute("version.rename", &json!({"id": id, "name": "v1 (renamed)"})).unwrap();
    r.write(&s, true);
    assert_eq!(copy(&root).versions.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["v1 (renamed)"]);
    r.finish();
    let _ = std::fs::remove_dir_all(&root);
}
