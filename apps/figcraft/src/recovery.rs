//! Recovery copies, so a crash, a forced quit or a power cut loses at most a few seconds of work.
//!
//! While the app runs, each document with unsaved changes is copied to `Recovery/<pid>/<uid>.figcraft`
//! in the config folder, in the background, at most every [`INTERVAL`] seconds, and the copy goes
//! once the document is saved or closed. The app holds a lock on its folder. At startup, the folders
//! whose lock is free belong to runs that ended without closing: their copies open again as
//! unsaved documents (`file.recover`). Each such copy **stays until this run has its own copy of
//! the document** (or it is saved or closed): until then it is the only copy on disk, and a run
//! that can't keep copies of its own leaves it for the next start. Then it goes, and its folder
//! with its last copy. Closing the app normally removes its own.
//!
//! A document that is one of your online files has a note beside its copy (`<uid>.online`, written
//! after the copy it describes): the server, the file's id, and who the app was in the file with
//! the last change of the server's it had; never a token. The next start joins that file again
//! as its owner (`collab.recoverFile`) and **keeps the copy until the outcome is known**: the
//! server has everything in it (the copy goes), the copy's own unconfirmed changes were put on
//! top of the server's file and sent, the copy was opened beside the file because both had
//! changed, or the file couldn't be joined and the copy was opened as any other. Whatever a copy
//! became is copied into this run's folder before the old copy is removed. Without the note, or
//! when the file is no longer recorded, the copy opens as any other.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use figcraft_engine::Session;
use serde_json::json;

/// Seconds between copies of a document that keeps changing.
pub const INTERVAL: f64 = 10.0;

const LOCK: &str = "lock";

/// The app's recovery, shared by its frame loop and its services (the panic path).
pub type Shared = Arc<Mutex<Option<Recovery>>>;

pub fn lock(r: &Shared) -> MutexGuard<'_, Option<Recovery>> {
    r.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct Recovery {
    dir: PathBuf,
    /// Held while the app runs: a free lock means the run that wrote the folder is gone.
    _lock: File,
    /// The revision each document was last copied at, and the write in flight.
    written: HashMap<u64, (u64, Option<JoinHandle<()>>)>,
    /// When (UI clock) the next copies are due.
    next: f64,
    /// Copies of online files from earlier runs, kept until their outcome is known.
    waiting: Vec<Waiting>,
}

/// A copy from an earlier run, kept until what it became is safe.
pub struct Waiting {
    until: Until,
    copy: PathBuf,
    /// The lock of the folder it is in: no other start takes the copy meanwhile (none for a copy
    /// left directly in the root by older versions).
    _lock: Option<Arc<File>>,
}

/// What a kept copy waits for.
enum Until {
    /// One of your online files, joined again as this session: its outcome.
    Session(u64),
    /// Opened as document `uid`: this run's own copy of it (or its save or close).
    Copied(u64),
}

/// What [`recover_all`] brought back.
#[derive(Default)]
pub struct Recovered {
    /// Documents opened from their copies.
    pub copies: usize,
    /// The copies kept until what they became is safe ([`Recovery::adopt`]).
    pub waiting: Vec<Waiting>,
}

impl Recovery {
    /// Take this run's folder under `root` (`None` when it can't be created or locked: no copies).
    /// A crashed run with the same process id may still hold its folder (its copies kept): then
    /// another name is taken.
    pub fn start(root: &Path) -> Option<Self> {
        let pid = std::process::id();
        (0..10).find_map(|n| {
            let dir = root.join(if n == 0 { pid.to_string() } else { format!("{pid}-{n}") });
            std::fs::create_dir_all(&dir).ok()?;
            let lock = File::options().write(true).create(true).truncate(false).open(dir.join(LOCK)).ok()?;
            lock.try_lock().ok()?;
            Some(Self { dir, _lock: lock, written: HashMap::new(), next: 0.0, waiting: Vec::new() })
        })
    }

    /// Keep earlier runs' copies until what they became is safe.
    pub fn adopt(&mut self, waiting: Vec<Waiting>) {
        self.waiting.extend(waiting);
    }

    /// Copy the documents that changed since their last copy and drop the copies of documents that
    /// were saved or closed. Returns how long until it needs to run again (`None`: nothing pending,
    /// an idle window can sleep).
    pub fn tick(&mut self, s: &Session, now: f64) -> Option<f64> {
        self.settle(s);
        // A join on its way settles within its timeouts: look again soon. A copy waiting for this
        // run's own is looked at again with the next copies.
        let joining = self.waiting.iter().any(|w| matches!(w.until, Until::Session(_)));
        let settling = if joining { Some(0.5) } else { (!self.waiting.is_empty()).then_some(INTERVAL) };
        let pending = s.docs.iter().any(|d| needs_copy(d) && self.written.get(&d.uid).is_none_or(|w| w.0 != d.revision));
        if now < self.next {
            return pending.then_some(self.next - now).or(settling);
        }
        self.write(s, false);
        self.next = now + INTERVAL;
        settling
    }

    /// Remove earlier runs' copies once what they became is safe: copied here (the write
    /// succeeded), saved or closed. An online file's copy goes once its outcome is known and every
    /// document is safe, since which of them it became isn't known.
    pub fn settle(&mut self, s: &Session) {
        let joined = |w: &Waiting| matches!(w.until, Until::Session(id) if !s.live.recovering(id));
        if self.waiting.iter().any(joined) {
            self.write(s, true);
        }
        let all_safe = s.docs.iter().all(|d| self.safe(d));
        let (done, waiting): (Vec<Waiting>, Vec<Waiting>) = std::mem::take(&mut self.waiting).into_iter().partition(|w| match w.until {
            Until::Session(_) => joined(w) && all_safe,
            Until::Copied(uid) => s.docs.iter().find(|d| d.uid == uid).is_none_or(|d| self.safe(d)),
        });
        self.waiting = waiting;
        done.into_iter().for_each(remove_kept);
    }

    /// Whether document `d`'s work is on disk: saved (or nothing to keep), or copied here with no
    /// write in flight.
    fn safe(&self, d: &figcraft_engine::DocState) -> bool {
        let written = self.written.get(&d.uid).is_some_and(|w| w.1.as_ref().is_none_or(JoinHandle::is_finished));
        !needs_copy(d) || (written && self.dir.join(format!("{}.figcraft", d.uid)).is_file())
    }

    /// Copy every document with unsaved changes; `wait`: before returning (after a panic).
    pub fn write(&mut self, s: &Session, wait: bool) {
        for d in &s.docs {
            let path = self.dir.join(format!("{}.figcraft", d.uid));
            let note = path.with_extension(ONLINE);
            if !needs_copy(d) {
                if self.written.remove(&d.uid).is_some() {
                    let _ = std::fs::remove_file(&path);
                    let _ = std::fs::remove_file(&note);
                }
                continue;
            }
            let entry = self.written.entry(d.uid).or_insert((0, None));
            let busy = entry.1.as_ref().is_some_and(|h| !h.is_finished());
            if entry.0 == d.revision || busy {
                continue;
            }
            let doc = d.doc.clone();
            // Which online file it is and what of the server's this very copy has (a line each:
            // the server, the file's id, then the session, client id and last change), while it
            // is one. Written after the copy, never before: a note must not promise more of the
            // server's changes than its copy holds.
            let online = s.online_of(d.uid).map(|(server, file, at)| format!("{server}\n{file}\n{} {} {}", at.session, at.client_id, at.seq));
            let job = move || {
                if !write_atomic(&path, &doc) {
                    return;
                }
                let _ = match online {
                    Some(text) => std::fs::write(&note, text),
                    None => std::fs::remove_file(&note),
                };
            };
            if wait {
                job();
                entry.1 = None;
            } else {
                entry.1 = std::thread::Builder::new().name("recovery".into()).spawn(job).ok();
            }
            entry.0 = d.revision;
        }
        // Closed documents.
        let open: Vec<u64> = s.docs.iter().map(|d| d.uid).collect();
        let dir = &self.dir;
        self.written.retain(|uid, _| {
            let keep = open.contains(uid);
            if !keep {
                let _ = std::fs::remove_file(dir.join(format!("{uid}.figcraft")));
                let _ = std::fs::remove_file(dir.join(format!("{uid}.{ONLINE}")));
            }
            keep
        });
    }

    /// A normal quit: the documents were saved or their changes discarded on purpose. (Earlier
    /// runs' copies still waiting for their outcome stay for the next start.)
    pub fn finish(self) {
        let Self { dir, _lock, written, waiting, .. } = self;
        // The documents earlier runs' copies became were saved or discarded too.
        waiting.into_iter().filter(|w| matches!(w.until, Until::Copied(_))).for_each(remove_kept);
        // Windows removes a folder only once nothing in it is open: copies being written first.
        for h in written.into_values().filter_map(|w| w.1) {
            let _ = h.join();
        }
        drop(_lock);
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Remove a kept copy and its note, and its folder with its last copy (while another is kept
/// there, its lock is held).
fn remove_kept(w: Waiting) {
    let _ = std::fs::remove_file(&w.copy);
    let _ = std::fs::remove_file(w.copy.with_extension(ONLINE));
    let last = w._lock.as_ref().is_some_and(|l| Arc::strong_count(l) == 1);
    let dir = w.copy.parent().map(Path::to_path_buf).filter(|_| last);
    drop(w);
    if let Some(dir) = dir {
        let _ = std::fs::remove_file(dir.join(LOCK));
        let _ = std::fs::remove_dir(dir);
    }
}

/// The extension of the note beside the copy of a document that is one of your online files.
const ONLINE: &str = "online";

/// The copy at `file` is of one of your online files: join that file again as its owner, keeping
/// the copy to compare with what arrives. The session, or `None` when the copy isn't one (or its
/// file isn't recorded any more, or is open already): then it opens as any other copy.
fn rejoin_online(file: &Path, s: &mut Session) -> Option<u64> {
    let note = std::fs::read_to_string(file.with_extension(ONLINE)).ok()?;
    let mut lines = note.lines().map(str::trim);
    let (server, id, at) = (lines.next()?, lines.next()?, lines.next()?);
    let mut at = at.split(' ').map(|n| n.parse::<u64>().ok());
    let (session, client, seq) = (at.next()??, at.next()??, at.next()??);
    let params = json!({"path": file.to_string_lossy(), "server": server, "file": id, "session": session, "client": client, "seq": seq});
    s.execute("collab.recoverFile", &params).ok()?.get("session")?.as_u64()
}

/// A document whose changes would be lost (a branch tab's edits are in its main file).
fn needs_copy(d: &figcraft_engine::DocState) -> bool {
    d.is_dirty() && d.branch_of.is_none()
}

/// Write `doc` to `path` through a temporary file; false when it couldn't be written (the copy
/// before it is still there).
fn write_atomic(path: &Path, doc: &figcraft_engine::doc::Document) -> bool {
    let Ok(bytes) = figcraft_format::save_bytes(doc, None) else { return false };
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, path).is_ok()
}

/// [`recover_all`], for a start that keeps nothing waiting: how many documents came back as
/// unsaved copies.
#[cfg(test)]
pub fn recover(root: &Path, s: &mut Session) -> usize {
    recover_all(root, s).copies
}

/// Open the copies left by runs that ended without closing (folders under `root` whose lock is
/// free, and copies directly in `root` from older versions) as unsaved documents. Each is kept
/// until this run has its own copy of the document ([`Recovered::waiting`]); a copy of one of your
/// online files, while its file is joined again. A copy that can't be read stays where it is.
pub fn recover_all(root: &Path, s: &mut Session) -> Recovered {
    let mut out = Recovered::default();
    let Ok(entries) = std::fs::read_dir(root) else { return out };
    let mut open = |file: &Path, lock: Option<&Arc<File>>, s: &mut Session| {
        if file.extension().is_none_or(|e| e != "figcraft") {
            return;
        }
        // One of your online files comes back from its server, its copy kept until then.
        if let Some((session, lock)) = lock.and_then(|l| Some((rejoin_online(file, s)?, l))) {
            out.waiting.push(Waiting { until: Until::Session(session), copy: file.to_path_buf(), _lock: Some(lock.clone()) });
            return;
        }
        let before: Vec<u64> = s.docs.iter().map(|d| d.uid).collect();
        if s.execute("file.recover", &json!({"path": file.to_string_lossy()})).is_ok() {
            out.copies += 1;
            if let Some(uid) = s.docs.iter().map(|d| d.uid).find(|u| !before.contains(u)) {
                out.waiting.push(Waiting { until: Until::Copied(uid), copy: file.to_path_buf(), _lock: lock.cloned() });
            }
        }
    };
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_dir() {
            open(&path, None, s);
            continue;
        }
        // Still running (another window of the app), or not ours to take.
        let Ok(lock) = File::options().write(true).create(true).truncate(false).open(path.join(LOCK)) else { continue };
        if lock.try_lock().is_err() {
            continue;
        }
        let lock = Arc::new(lock);
        let files: Vec<PathBuf> = std::fs::read_dir(&path).into_iter().flatten().flatten().map(|f| f.path()).collect();
        for f in &files {
            if f.extension().is_some_and(|e| e == "tmp") {
                // A copy cut off mid-write; the one before it is still there.
                let _ = std::fs::remove_file(f);
            } else {
                open(f, Some(&lock), s);
            }
        }
        // The notes go with their copies (one whose copy stays, stays with it).
        for f in files.iter().filter(|f| f.extension().is_some_and(|e| e == ONLINE) && !f.with_extension("figcraft").exists()) {
            let _ = std::fs::remove_file(f);
        }
        // Empty now unless a copy couldn't be read, or is kept (its lock held).
        if Arc::strong_count(&lock) == 1 {
            drop(lock);
            let _ = std::fs::remove_file(path.join(LOCK));
            let _ = std::fs::remove_dir(&path);
        }
    }
    out
}
