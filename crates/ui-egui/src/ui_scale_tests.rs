//! The interface size, driven through egui headless on screens of different system scales: Auto
//! leaves the system's scale alone, a fixed size comes out the same on every screen, and the
//! Preferences drop-down tells a screen reader its name and value.
//!
//! The test window's size is fixed in points, so it grows with the zoom here; the limit for a small
//! window is tested on its own in `ui_scale::tests`, and the sizes below stay inside it.

use egui::accesskit::Role;
use egui::{RawInput, ViewportId, ViewportInfo};
use figcraft_engine::Session;
use serde_json::json;

use crate::layers_drag_tests::Ui;

/// One frame on a screen whose system scale is `native` (a real window reports it every frame).
fn frame_at(u: &mut Ui, native: f32) -> egui::FullOutput {
    let mut raw = RawInput::default();
    raw.viewports.insert(ViewportId::ROOT, ViewportInfo { native_pixels_per_point: Some(native), ..Default::default() });
    u.frame_raw(raw)
}

/// Frames on that screen, enough for a new zoom to take effect.
fn settle(u: &mut Ui, native: f32) {
    for _ in 0..4 {
        frame_at(u, native);
    }
}

const SCREENS: [f32; 5] = [1.0, 1.25, 1.5, 2.0, 3.0];

#[test]
fn auto_leaves_the_systems_scale_and_the_menu_can_say_what_it_is() {
    for native in SCREENS {
        let mut u = Ui::new(Session::new());
        settle(&mut u, native);
        assert_eq!(u.ctx.zoom_factor(), 1.0, "{native}");
        assert_eq!(u.ctx.pixels_per_point(), native, "{native}");
        assert_eq!(u.app.ui.ui_native, native, "what Auto shows in the menu: {native}");
    }
}

#[test]
fn a_fixed_size_draws_the_same_on_every_screen_and_auto_brings_the_systems_scale_back() {
    for native in SCREENS {
        let mut u = Ui::new(Session::new());
        for size in [0.75_f64, 1.0, 1.5] {
            assert!(u.app.run("pref.uiScale", json!({"scale": size})).is_some());
            settle(&mut u, native);
            let drawn = u.ctx.pixels_per_point();
            assert!((drawn - size as f32).abs() < 1e-4, "{size} on a {native}× screen drew at {drawn}");
            assert!(!u.app.ui.ui_scale.limited, "{size} on {native}");
        }
        assert!(u.app.run("pref.uiScale", json!({"scale": "auto"})).is_some());
        settle(&mut u, native);
        assert_eq!((u.ctx.zoom_factor(), u.ctx.pixels_per_point()), (1.0, native), "{native}");
    }
}

#[test]
fn the_interface_size_menu_has_a_name_a_value_and_a_state_for_a_screen_reader() {
    let mut u = Ui::new(Session::new());
    u.app.ui.preferences = true;
    u.ctx.enable_accesskit();
    let combo = |u: &mut Ui| {
        settle(u, 2.0);
        let out = frame_at(u, 2.0);
        let nodes: Vec<_> = out.platform_output.accesskit_update.expect("an AccessKit update").nodes.into_iter().map(|(_, n)| n).collect();
        let n = nodes.iter().find(|n| n.role() == Role::ComboBox && n.label() == Some("Interface size")).expect("an Interface size menu");
        (n.value().map(str::to_owned), n.is_expanded())
    };
    // Auto says the size it draws at: a 2× screen is 200 %.
    assert_eq!(combo(&mut u), (Some("Auto (200%)".to_owned()), Some(false)));
    u.app.run("pref.uiScale", json!({"scale": 1.25}));
    assert_eq!(combo(&mut u), (Some("125%".to_owned()), Some(false)));
}
