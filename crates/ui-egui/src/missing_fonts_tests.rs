use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::import_report::Report;
use crate::layers_drag_tests::Ui;

/// A saved file with text in `families` (the first two layers share the first family).
fn saved(name: &str, families: &[&str]) -> String {
    let mut s = Session::new();
    for (i, f) in families.iter().chain(families.first()).enumerate() {
        s.execute("text.create", &json!({"text": "Hamburgefonstiv", "x": 0, "y": 40 * i, "props": {"fontFamily": f}})).unwrap();
    }
    let path = std::env::temp_dir().join(format!("figcraft-missing-fonts-{name}-{}.figcraft", std::process::id()));
    let path = path.to_string_lossy().into_owned();
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    path
}

/// The installed fonts are indexed, as the desktop app does at startup; false when this machine
/// opted out (nothing would ever count as known).
fn fonts_indexed() -> bool {
    figcraft_text::scan_system_fonts();
    figcraft_text::system_fonts_scanned()
}

fn listed(u: &Ui) -> Value {
    u.app.inspect()["missingFonts"].clone()
}

#[test]
fn opening_a_file_lists_its_missing_fonts_and_not_the_installed_ones() {
    if !fonts_indexed() {
        eprintln!("skipped: system fonts opted out");
        return;
    }
    let installed = figcraft_text::family_names().into_iter().next();
    let mut families = vec!["FigCraft Missing UI"];
    families.extend(installed.as_deref());
    let path = saved("open", &families);
    let mut u = Ui::new(Session::new());
    // Programmatic opens (control channel, MCP, scripts) never open the window.
    u.app.run("file.open", json!({"path": path}));
    u.frame(vec![]);
    assert_eq!(listed(&u), Value::Null);

    u.app.run_user("file.open", json!({"path": path}));
    std::fs::remove_file(&path).unwrap();
    u.frame(vec![]);
    assert_eq!(listed(&u), json!({"families": [{"family": "FigCraft Missing UI", "layers": 2}]}));
    u.frame(vec![]);
    let shown = u.frame_output(vec![]).shapes;
    assert!(crate::import_report_tests::shown_texts(shown).iter().any(|t| t == "2 text layers"));

    // It goes with its file.
    u.app.run("file.close", json!({}));
    u.frame(vec![]);
    assert_eq!(listed(&u), Value::Null);
}

#[test]
fn a_file_without_missing_fonts_opens_no_window() {
    if !fonts_indexed() {
        eprintln!("skipped: system fonts opted out");
        return;
    }
    let Some(installed) = figcraft_text::family_names().into_iter().next() else {
        eprintln!("skipped: no fonts on this machine");
        return;
    };
    let path = saved("clean", &[&installed]);
    let mut u = Ui::new(Session::new());
    u.app.run_user("file.open", json!({"path": path}));
    std::fs::remove_file(&path).unwrap();
    u.frame(vec![]);
    assert_eq!(listed(&u), Value::Null);
    assert!(u.app.ui.font_checks.is_empty(), "the check ran");
}

#[test]
fn it_waits_for_the_import_report_to_close() {
    if !fonts_indexed() {
        eprintln!("skipped: system fonts opted out");
        return;
    }
    let path = saved("report", &["FigCraft Missing Report"]);
    let mut u = Ui::new(Session::new());
    u.app.run_user("file.open", json!({"path": path}));
    std::fs::remove_file(&path).unwrap();
    let mut report =
        Report::from_result("file.open", &json!({}), &json!({"warnings": ["Synthetic note"], "summary": {"name": "Synthetic"}})).unwrap();
    report.uid = u.app.session.ds().unwrap().uid;
    u.app.ui.import_report = Some(report);
    u.frame(vec![]);
    assert_eq!(listed(&u), Value::Null, "the import report comes first");
    u.app.ui.import_report = None;
    u.frame(vec![]);
    assert_eq!(listed(&u)["families"][0]["family"], "FigCraft Missing Report");
}

#[test]
fn text_menu_shows_the_window_for_the_file_in_front_even_with_nothing_missing() {
    let mut u = Ui::new(Session::new());
    assert!(u.app.run("text.missingFonts", json!({})).is_some());
    assert_eq!(listed(&u), json!({"families": []}));
    u.frame(vec![]);
    let shown = u.frame_output(vec![]).shapes;
    assert!(crate::import_report_tests::shown_texts(shown).iter().any(|t| t == "All the fonts this file uses are installed"));
    let escape =
        vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }];
    u.frame(escape);
    assert_eq!(listed(&u), Value::Null, "Escape closes it");
}
