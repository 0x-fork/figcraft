//! The Actions palette, driven headless: invoking records recents (newest first, capped),
//! sections group commands by menu, the query filters, and gone entries drop out of Recents.

use figcraft_engine::Session;

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
