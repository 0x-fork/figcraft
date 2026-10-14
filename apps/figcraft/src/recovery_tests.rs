//! Recovery copies: a run that ends without closing leaves copies of its unsaved documents that the
//! next start opens again (a running app's are left alone); a normal quit leaves nothing.

use figcraft_engine::Session;
use serde_json::json;

use crate::recovery::{Recovery, recover, recover_all};

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
    assert!(std::fs::read_dir(&root).unwrap().next().is_some(), "the copy stays until a run has its own");
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

/// A run that ended without closing, leaving a copy of `s`'s unsaved document: its folder (as
/// another process's).
fn crashed(s: &Session, root: &std::path::Path) -> std::path::PathBuf {
    let mut r = Recovery::start(root).unwrap();
    r.write(s, true);
    drop(r);
    let left = root.join("crashed");
    std::fs::rename(root.join(std::process::id().to_string()), &left).unwrap();
    left
}

fn copies_in(d: &std::path::Path) -> usize {
    std::fs::read_dir(d).map_or(0, |d| d.flatten().filter(|f| f.path().extension().is_some_and(|e| e == "figcraft")).count())
}

/// The next start: the copies come back, and this run's recovery keeps them until it covers them.
fn restart(root: &std::path::Path) -> (Session, Recovery) {
    let mut next = Session::empty();
    let recovered = recover_all(root, &mut next);
    assert_eq!(recovered.copies, 1);
    let mut r = Recovery::start(root).unwrap();
    r.adopt(recovered.waiting);
    (next, r)
}

#[test]
fn a_recovered_copy_stays_until_this_run_has_its_own_copy() {
    let root = root("handoff");
    let left = crashed(&edited(), &root);
    let (next, mut r) = restart(&root);
    assert_eq!(copies_in(&left), 1, "the only copy on disk until this run writes one");
    r.write(&next, true);
    r.tick(&next, 100.0);
    assert!(!left.exists(), "covered by this run's copy, the old one goes");
    assert_eq!(copies_in(&root.join(std::process::id().to_string())), 1);
    r.finish();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_recovered_copy_stays_while_its_replacement_cant_be_written() {
    let root = root("blocked");
    let left = crashed(&edited(), &root);
    let (next, mut r) = restart(&root);
    // Something is in the way of this run's copy: writing it fails.
    let mine = root.join(std::process::id().to_string()).join(format!("{}.figcraft", next.docs[0].uid));
    std::fs::create_dir_all(&mine).unwrap();
    r.write(&next, true);
    r.tick(&next, 100.0);
    assert_eq!(copies_in(&left), 1, "kept: the work is nowhere else on disk");
    // Once it can be written, the old copy goes.
    std::fs::remove_dir(&mine).unwrap();
    let mut next = next;
    next.execute("shape.rectangle", &json!({"x": 1, "y": 1, "width": 2, "height": 2})).unwrap();
    r.write(&next, true);
    r.tick(&next, 200.0);
    assert!(!left.exists());
    r.finish();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn without_a_recovery_of_its_own_the_copy_stays_for_the_next_start() {
    let root = root("nocopies");
    crashed(&edited(), &root);
    let mut next = Session::empty();
    let recovered = recover_all(&root, &mut next);
    assert_eq!(recovered.copies, 1);
    // This run couldn't take a folder of its own: it ends (or crashes) with nothing copied.
    drop(recovered);
    let mut after = Session::empty();
    assert_eq!(recover(&root, &mut after), 1, "the work comes back again");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_recovered_copy_goes_once_its_document_is_saved_or_closed() {
    for close in [false, true] {
        let root = root(if close { "closed" } else { "saved" });
        let left = crashed(&edited(), &root);
        let (mut next, mut r) = restart(&root);
        if close {
            next.docs.clear();
        } else {
            next.ds_mut().unwrap().mark_saved();
        }
        r.tick(&next, 0.0);
        assert!(!left.exists(), "close {close}");
        r.finish();
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn a_normal_quit_removes_the_recovered_copies_it_took_over() {
    let root = root("quitafter");
    let left = crashed(&edited(), &root);
    let (_, r) = restart(&root);
    // Quitting asked to save or discard every document.
    r.finish();
    assert!(!left.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_run_whose_folder_name_is_still_held_takes_another() {
    let root = root("pidreuse");
    // A crashed run with this process's id whose folder the start still holds (its copy waits).
    let held = root.join(std::process::id().to_string());
    std::fs::create_dir_all(&held).unwrap();
    let lock = std::fs::File::create(held.join("lock")).unwrap();
    lock.try_lock().unwrap();
    let mut r = Recovery::start(&root).expect("a folder of its own");
    let s = edited();
    r.write(&s, true);
    assert_eq!(copies_in(&held), 0);
    let dirs = std::fs::read_dir(&root).unwrap().flatten().filter(|e| e.path() != held).count();
    assert_eq!(dirs, 1);
    r.finish();
    drop(lock);
    let _ = std::fs::remove_dir_all(&root);
}
