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

fn slow_write(path: std::path::PathBuf) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let _ = std::fs::write(path, b"stale");
    })
}

#[test]
fn saving_or_closing_waits_for_a_slow_write_so_the_copy_does_not_come_back() {
    let root = root("slow-cleanup");
    let mut s = edited();
    let mut r = Recovery::start(&root).unwrap();
    let dir = std::fs::read_dir(&root).unwrap().flatten().next().unwrap().path();
    let uid = s.docs[0].uid;
    let copy = dir.join(format!("{uid}.figcraft"));
    r.hold(uid, 1, slow_write(copy.clone()));
    s.ds_mut().unwrap().mark_saved();
    r.write(&s, false);
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(!copy.exists(), "a saved document's copy stays removed");
    // Closed: the same.
    let s = edited();
    let uid = s.docs[0].uid;
    let copy = dir.join(format!("{uid}.figcraft"));
    r.hold(uid, 1, slow_write(copy.clone()));
    r.write(&Session::empty(), false);
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(!copy.exists(), "a closed document's copy stays removed");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_emergency_save_waits_for_the_write_in_flight_and_copies_the_newest_revision() {
    let root = root("slow-emergency");
    let s = edited();
    let mut r = Recovery::start(&root).unwrap();
    let dir = std::fs::read_dir(&root).unwrap().flatten().next().unwrap().path();
    let uid = s.docs[0].uid;
    let copy = dir.join(format!("{uid}.figcraft"));
    r.hold(uid, 0, slow_write(copy.clone()));
    r.write(&s, true);
    assert!(std::fs::read(&copy).is_ok_and(|b| b != b"stale"), "the newest revision replaced the slow write");
    let _ = std::fs::remove_dir_all(&root);
}
