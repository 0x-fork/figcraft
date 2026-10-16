//! The Actions palette, driven headless: invoking records recents (newest first, capped),
//! sections group commands by menu, the query filters, gone entries drop out of Recents, a used
//! plugin resolves back to a row, and a press outside the panel closes it.

use egui::Pos2;
use figcraft_engine::Session;
use serde_json::json;

use crate::layers_drag_tests::Ui;

#[test]
fn invoke_records_recents_newest_first_and_capped() {
    let mut u = Ui::new(Session::new());
    for id in [
        "view.zoomIn",
        "view.zoomOut",
        "edit.copyAsText",
        "style.list",
        "edit.findReplace",
        "edit.copyAsCss",
        "view.zoomIn",
    ] {
        crate::menus::invoke(&mut u.app, id);
    }
    let r = &u.app.session.prefs.recent_actions;
    assert_eq!(r.len(), figcraft_engine::MAX_RECENT_ACTIONS, "recents are capped: {r:?}");
    assert_eq!(r[0], "command:view.zoomIn", "a re-run moves to the front: {r:?}");
    assert_eq!(r.iter().filter(|k| k.as_str() == "command:view.zoomIn").count(), 1, "no duplicates: {r:?}");
    assert!(!r.contains(&"command:view.zoomOut".to_string()), "the oldest drops out: {r:?}");
}

#[test]
fn sections_group_by_menu_in_catalog_order() {
    let groups = crate::menus::command_sections("");
    assert!(!groups.is_empty(), "the catalog has commands");
    assert_eq!(groups[0].0, "File", "catalog order starts at File");
    for (group, rows) in &groups {
        assert!(!rows.is_empty(), "{group} has no rows");
        for row in rows {
            assert!(row.key.starts_with("command:"), "unexpected key {}", row.key);
        }
    }
    let names: Vec<&str> = groups.iter().map(|(g, _)| g.as_str()).collect();
    assert!(names.contains(&"File") && names.contains(&"Edit"), "menus group: {names:?}");
    assert!(!names.iter().any(|g| g.starts_with("Plugins")), "plugins have their own tab");
}

#[test]
fn query_filters_to_matches() {
    let groups = crate::menus::command_sections("save");
    let labels: Vec<&str> = groups.iter().flat_map(|(_, rows)| rows.iter().map(|r| r.label.as_str())).collect();
    assert!(!labels.is_empty(), "something matches 'save'");
    assert!(labels.iter().all(|l| l.to_lowercase().contains("save")), "only matches: {labels:?}");
    assert!(crate::menus::command_sections("zzz-no-such-action").iter().all(|(_, r)| r.is_empty()));
}

#[test]
fn gone_entries_drop_out_of_recents() {
    let u = Ui::new(Session::new());
    assert!(crate::menus::recent_row(&u.app, "command:nope.nope", "").is_none(), "gone commands drop out");
    assert!(crate::menus::recent_row(&u.app, "plugin:nope.nope/nope", "").is_none(), "gone plugins drop out");
    assert!(crate::menus::recent_row(&u.app, "widget:nope", "").is_none(), "gone widgets drop out");
}

/// A used plugin (API 4, several commands) resolves back to a Recents row.
#[test]
fn used_plugin_resolves_to_a_recent_row() {
    let mut u = Ui::new(Session::new());
    let plugin: figcraft_engine::plugins::InstalledPlugin = serde_json::from_value(json!({
        "manifest": {
            "id": "dev.local.solar-icons",
            "name": "Solar icons",
            "main": "solar_icons.wasm",
            "api": 4,
            "commands": [
                {"id": "panel", "label": "Panel"},
                {"id": "grid", "label": "Grid"},
            ],
        },
    }))
    .unwrap();
    u.app.session.prefs.plugins.push(plugin);
    crate::menus::record_recent(&mut u.app, "plugin:dev.local.solar-icons/panel");
    let row = crate::menus::recent_row(&u.app, "plugin:dev.local.solar-icons/panel", "")
        .expect("a used plugin stays in Recents");
    assert_eq!(row.label, "Solar icons — Panel");
    assert!(crate::menus::recent_row(&u.app, "plugin:dev.local.solar-icons/panel", "zzz").is_none(), "the query filters recents");
}

/// A press outside the panel closes it; the opening press is spared once.
#[test]
fn outside_press_closes_the_panel() {
    let mut u = Ui::new(Session::new());
    u.app.ui.palette = Some(String::new());
    u.app.ui.palette_armed = true;
    // The opening press: spared, and disarmed.
    u.button(Pos2::new(720.0, 20.0), true);
    u.button(Pos2::new(720.0, 20.0), false);
    assert!(u.app.ui.palette.is_some(), "the opening press is spared");
    // The next press outside: closed.
    u.button(Pos2::new(720.0, 20.0), true);
    u.button(Pos2::new(720.0, 20.0), false);
    assert!(u.app.ui.palette.is_none(), "an outside press closes the panel");
    // One click is enough: the spare expires on frames without a press.
    u.app.ui.palette = Some(String::new());
    u.app.ui.palette_armed = true;
    u.frame(vec![]);
    u.button(Pos2::new(720.0, 20.0), true);
    u.button(Pos2::new(720.0, 20.0), false);
    assert!(u.app.ui.palette.is_none(), "a single outside press closes the panel");
}
