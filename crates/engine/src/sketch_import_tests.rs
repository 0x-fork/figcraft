//! `.sketch` files through the file commands. The importer's own tests (fixtures built in code)
//! are in `figcraft_format::sketch`.

use std::io::Write;

use base64::Engine;
use serde_json::{Value, json};

use crate::Session;

/// A minimal `.sketch` zip with one page per `(id, name)`, saved by `app_version`.
fn sketch(app_version: &str, pages: &[(&str, &str)]) -> Vec<u8> {
    let refs: Vec<Value> = pages.iter().map(|(id, _)| json!({"_class": "MSJSONFileReference", "_ref": format!("pages/{id}")})).collect();
    let mut out = Vec::new();
    {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("document.json", o).unwrap();
        z.write_all(json!({"_class": "document", "pages": refs}).to_string().as_bytes()).unwrap();
        z.start_file("meta.json", o).unwrap();
        z.write_all(json!({"app": "com.bohemiancoding.sketch3", "appVersion": app_version, "version": 103}).to_string().as_bytes()).unwrap();
        for (id, name) in pages {
            z.start_file(format!("pages/{id}.json"), o).unwrap();
            z.write_all(json!({"_class": "page", "do_objectID": id, "name": name, "layers": []}).to_string().as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    out
}

fn temp(name: &str, bytes: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("figcraft-sketch-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    (dir, path)
}

fn page_names(s: &Session) -> Vec<String> {
    let doc = s.doc().unwrap();
    doc.pages.iter().map(|p| doc.node(*p).unwrap().name.clone()).collect()
}

#[test]
fn opened_sketch_files_have_their_pages_and_no_path() {
    let (dir, path) = temp("design.sketch", &sketch("50.2", &[("A", "Screens"), ("B", "Symbols")]));
    let mut s = Session::new();
    let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
    assert!(r.get("summary").is_some());
    assert_eq!(page_names(&s), ["Screens", "Symbols"]);
    // Never saved over: the document has no path, so Save asks for one.
    assert!(s.docs[s.active].path.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sketch_import_appends_pages() {
    let (dir, path) = temp("more.sketch", &sketch("83.2", &[("A", "Imported")]));
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let before = page_names(&s).len();
    s.execute("file.import", &json!({"path": path.to_string_lossy()})).unwrap();
    let names = page_names(&s);
    assert_eq!(names.len(), before + 1);
    assert_eq!(names.last().unwrap(), "Imported");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sketch_data_opens_with_a_summary() {
    let data = base64::engine::general_purpose::STANDARD.encode(sketch("50.2", &[("A", "Page")]));
    let mut s = Session::new();
    let r = s.execute("file.open", &json!({"data": data, "name": "x.sketch"})).unwrap();
    assert!(r.get("summary").is_some());
    assert_eq!(page_names(&s), ["Page"]);
}

#[test]
fn old_and_damaged_sketch_files_fail_cleanly() {
    let mut s = Session::new();
    let (dir, path) = temp("old.sketch", &sketch("47.1", &[("A", "P")]));
    let e = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap_err().to_string();
    assert!(e.contains("older than 48"), "{e}");
    let mut broken = sketch("50.2", &[("A", "P")]);
    broken.truncate(broken.len() / 2);
    std::fs::write(&path, &broken).unwrap();
    assert!(s.execute("file.open", &json!({"path": path.to_string_lossy()})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
