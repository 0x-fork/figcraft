//! `.sketch` import: detection, the archive, the version cut and pages, on fixtures built in code.

use serde_json::json;

use super::fixture::*;
use super::*;

fn page_names(doc: &Document) -> Vec<String> {
    doc.pages.iter().map(|p| doc.node(*p).unwrap().name.clone()).collect()
}

#[test]
fn detects_sketch_and_not_others() {
    let z = sketch_zip("50.2", 103, document(&["P1"]), &[page("P1", "Page 1", vec![])], &[]);
    assert!(is_sketch(&z));
    assert!(!is_sketch(b"fig-kiwi...."));
    assert!(!is_sketch(b"{}"));
    assert!(!is_sketch(b""));
    // A zip that isn't a Sketch file.
    let mut other = Vec::new();
    {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut other));
        w.start_file("canvas.fig", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"x").unwrap();
        w.finish().unwrap();
    }
    assert!(!is_sketch(&other));
}

#[test]
fn pages_become_pages_in_document_order() {
    let z = sketch_zip("50.2", 103, document(&["B", "A"]), &[page("A", "Alpha", vec![]), page("B", "Beta", vec![])], &[]);
    let (doc, _) = import(&z, "f").unwrap();
    assert_eq!(page_names(&doc), ["Beta", "Alpha"]);
}

#[test]
fn older_than_48_is_refused() {
    let z = sketch_zip("47.1", 95, document(&["P"]), &[page("P", "P", vec![])], &[]);
    let e = import(&z, "f").unwrap_err().to_string();
    assert!(e.contains(TOO_OLD), "{e}");
    let mut sqlite = b"SQLite format 3\0".to_vec();
    sqlite.resize(512, 0);
    assert!(is_sketch(&sqlite));
    assert!(import(&sqlite, "f").unwrap_err().to_string().contains(TOO_OLD));
}

#[test]
fn archived_text_without_meta_version_is_refused() {
    let text = json!({
        "_class": "text", "do_objectID": "T", "name": "T",
        "frame": {"_class": "rect", "x": 0, "y": 0, "width": 10, "height": 10},
        "attributedString": {"_class": "MSAttributedString", "archivedAttributedString": {"_archive": "YnBsaXN0MDA="}},
    });
    let z = raw_zip(&document(&["P"]), &json!({"app": "com.bohemiancoding.sketch3"}), &[page("P", "P", vec![text])], &[]);
    let e = import(&z, "f").unwrap_err().to_string();
    assert!(e.contains(TOO_OLD), "{e}");
}

#[test]
fn newer_than_tested_imports_with_one_warning() {
    let z = sketch_zip("2099.1", NEWEST_TESTED_VERSION + 1, document(&["P"]), &[page("P", "P", vec![])], &[]);
    let (_, warnings) = import(&z, "f").unwrap();
    assert_eq!(warnings.iter().filter(|w| w.contains("newer Sketch")).count(), 1, "{warnings:?}");
}

#[test]
fn junk_never_panics() {
    let z = sketch_zip("50.2", 103, document(&["P"]), &[page("P", "P", vec![rect("R", 0.0, 0.0, 10.0, 10.0)])], &[]);
    for cut in (0..z.len()).step_by(7) {
        let _ = import(&z[..cut], "f");
    }
    let mut seed = 0x2545_f491_u32;
    for _ in 0..200 {
        let mut b = z.clone();
        for _ in 0..8 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let i = seed as usize % b.len();
            b[i] = (seed >> 8) as u8;
        }
        let _ = import(&b, "f");
    }
    for doc in [json!([]), json!(null), json!({"pages": 5}), json!({"pages": [{"_ref": 7}, {"_ref": "pages/../../x"}]})] {
        let _ = import(&sketch_zip("50.2", 103, doc, &[json!(5), json!({"do_objectID": "Q", "layers": "x"})], &[]), "f");
    }
    let _ = import(&raw_zip(&document(&["P"]), &json!("x"), &[], &[]), "f");
}

#[test]
fn deep_nesting_does_not_fail_the_file() {
    let mut deep = rect("R", 0.0, 0.0, 1.0, 1.0);
    for i in 0..300 {
        deep = group(&format!("G{i}"), 0.0, 0.0, 1.0, 1.0, vec![deep]);
    }
    let z = sketch_zip("50.2", 103, document(&["A", "B"]), &[page("A", "Deep", vec![deep]), page("B", "Normal", vec![])], &[]);
    let (doc, warnings) = import(&z, "f").unwrap();
    assert!(page_names(&doc).contains(&"Normal".to_string()));
    assert!(!warnings.is_empty());
}

#[test]
fn members_over_budget_are_skipped() {
    let z = sketch_zip("50.2", 103, document(&["A", "B"]), &[page("A", "Big", vec![]), page("B", "Small", vec![])], &[]);
    let (doc, warnings) = import_within(&z, "f", 200).unwrap();
    // Each page JSON is larger than 200 bytes: both are skipped, with a warning; nothing panics.
    assert!(warnings.iter().any(|w| w.contains("too large")), "{warnings:?}");
    assert!(!page_names(&doc).contains(&"Big".to_string()));
}
