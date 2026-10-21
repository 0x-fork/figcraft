//! `.sketch` files through the file commands. The importer's own tests (fixtures built in code)
//! are in `figcraft_format::sketch`.

use std::io::Write;

use base64::Engine;
use serde_json::{Value, json};

use crate::Session;

/// A minimal `.sketch` zip with one page per `(id, name)`, saved by `app_version`.
fn sketch(app_version: &str, pages: &[(&str, &str)]) -> Vec<u8> {
    sketch_with(app_version, pages, &[])
}

/// [`sketch`] with `layers` on the first page.
fn sketch_with(app_version: &str, pages: &[(&str, &str)], layers: &[Value]) -> Vec<u8> {
    let refs: Vec<Value> = pages.iter().map(|(id, _)| json!({"_class": "MSJSONFileReference", "_ref": format!("pages/{id}")})).collect();
    let mut out = Vec::new();
    {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("document.json", o).unwrap();
        z.write_all(json!({"_class": "document", "pages": refs}).to_string().as_bytes()).unwrap();
        z.start_file("meta.json", o).unwrap();
        z.write_all(json!({"app": "com.bohemiancoding.sketch3", "appVersion": app_version, "version": 103}).to_string().as_bytes()).unwrap();
        for (i, (id, name)) in pages.iter().enumerate() {
            let ls = if i == 0 { layers.to_vec() } else { Vec::new() };
            z.start_file(format!("pages/{id}.json"), o).unwrap();
            z.write_all(json!({"_class": "page", "do_objectID": id, "name": name, "layers": ls}).to_string().as_bytes()).unwrap();
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

fn shape(class: &str, name: &str, frame: [f64; 4], extra: Value) -> Value {
    let mut l = json!({"_class": class, "do_objectID": name, "name": name, "isVisible": true, "resizingConstraint": 63,
        "frame": {"x": frame[0], "y": frame[1], "width": frame[2], "height": frame[3]}, "style": {"fills": []}});
    for (k, v) in extra.as_object().unwrap() {
        l[k] = v.clone();
    }
    l
}

fn fill(grey: f64) -> Value {
    json!({"style": {"fills": [{"isEnabled": true, "fillType": 0, "color": {"red": grey, "green": grey, "blue": grey, "alpha": 1}}]}})
}

/// Sketch draws a mask's own fill under what it clips; the clipped layers show inside it.
#[test]
fn sketch_masks_show_their_content() {
    let oval = shape("oval", "Ring", [0.0, 0.0, 60.0, 60.0], json!({"booleanOperation": -1, "points": []}));
    let mut mask = shape("shapeGroup", "Circle", [0.0, 0.0, 60.0, 60.0], fill(0.8));
    mask["hasClippingMask"] = json!(true);
    mask["layers"] = json!([oval]);
    let pts = ["{0, 0}", "{1, 0}", "{1, 1}", "{0, 1}"].map(|p| json!({"point": p, "curveFrom": p, "curveTo": p, "curveMode": 1}));
    let body = shape("shapePath", "Square", [0.0, 0.0, 20.0, 20.0], json!({"points": pts, "isClosed": true, "booleanOperation": -1}));
    let mut content = shape("shapeGroup", "Body", [20.0, 20.0, 20.0, 20.0], fill(0.2));
    content["layers"] = json!([body]);
    let icon = shape("group", "Icon", [0.0, 0.0, 60.0, 60.0], json!({"layers": [mask, content]}));
    let data = base64::engine::general_purpose::STANDARD.encode(sketch_with("50.2", &[("A", "Page")], &[icon]));
    let mut s = Session::new();
    s.execute("file.open", &json!({"data": data, "name": "x.sketch"})).unwrap();
    let doc = s.doc().unwrap();
    let icon = doc.nodes().find(|n| n.name == "Icon").unwrap().id;
    let pix = figcraft_render::render_node(doc, icon, 1.0).unwrap();
    let px = |x: u32, y: u32| pix.pixel(x, y).unwrap();
    // The square in the middle, dark grey.
    let c = px(30, 30);
    assert!(c.alpha() > 200 && c.red() < 100, "content: {c:?}");
    // The mask's own fill around it, light grey; outside the circle nothing.
    let m = px(30, 8);
    assert!(m.alpha() > 200 && m.red() > 150, "mask fill: {m:?}");
    assert!(px(2, 2).alpha() < 50);
}

/// Family names guessed from PostScript names find the installed family whatever its spaces.
#[test]
fn sketch_font_families_match_installed_ones() {
    let installed = ["PingFang SC".to_string(), "Helvetica Neue".to_string(), "IBM Plex Sans".to_string()];
    let m = |f: &str| crate::cmd::file::installed_family(f, &installed);
    assert_eq!(m("Ping Fang SC").as_deref(), Some("PingFang SC"));
    assert_eq!(m("Helvetica Neue"), None, "already right");
    assert_eq!(m("IBMPlex Sans").as_deref(), Some("IBM Plex Sans"));
    assert_eq!(m("Comic Sans"), None);
}
