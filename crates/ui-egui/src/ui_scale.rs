//! The interface size (Preferences ▸ Interface size, `pref.uiScale`): how the saved choice becomes
//! egui's zoom factor.
//!
//! egui draws at `pixels_per_point = zoom factor × the system's scale`. Auto leaves the zoom at 1 so
//! the system's scale rules. A fixed size is absolute: 100 % is one screen pixel per point on every
//! system, so the zoom is the size divided by the system's scale (a Retina Mac at 2× needs 0.5 to
//! show 100 %). A fixed size is held back while the window is too small to lay the interface out at
//! it; the saved choice never changes.

/// The smallest window, in points, the interface is laid out for: the main window's minimum size.
pub const FIT_POINTS: (f32, f32) = (800.0, 500.0);

/// The zoom factors egui's own settings window allows.
const ZOOM_RANGE: (f32, f32) = (0.1, 10.0);

/// What to give egui for a saved choice on this screen and window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    /// For `Context::set_zoom_factor`.
    pub zoom: f32,
    /// Physical pixels per point once drawn (the zoom times the system's scale).
    pub drawn: f32,
    /// A fixed size was chosen but the window is too small for it, so a smaller one is drawn.
    pub limited: bool,
}

/// The plan for `choice` (`None`: Auto), the system's scale `native` and the window's size in
/// physical pixels `window_px`. An unknown, zero or not finite value reads as no information: a
/// system scale of 1 and a window of any size.
pub fn plan(choice: Option<f64>, native: Option<f32>, window_px: Option<(f32, f32)>) -> Plan {
    let native = native.filter(|n| n.is_finite() && *n > 0.0).unwrap_or(1.0);
    let Some(want) = choice.map(|c| c as f32).filter(|c| c.is_finite() && *c > 0.0) else {
        return Plan { zoom: 1.0, drawn: native, limited: false };
    };
    let fit =
        window_px.filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0).map(|(w, h)| (w / FIT_POINTS.0).min(h / FIT_POINTS.1));
    // Never below 1 (or the size asked, when that is smaller): a tiny window doesn't shrink the interface further.
    let target = match fit {
        Some(fit) if want > fit => fit.max(want.min(1.0)),
        _ => want,
    };
    let zoom = (target / native).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
    let drawn = zoom * native;
    Plan { zoom, drawn, limited: drawn < want - 0.005 }
}

/// A size as the percentage Preferences shows (1.25 → 125).
pub fn percent(scale: f32) -> u32 {
    (scale * 100.0).round().clamp(0.0, 100_000.0) as u32
}

/// The fixed sizes Preferences offers, after Auto.
pub const SIZES: [f64; 7] = [0.75, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0];

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: Option<(f32, f32)> = Some((5000.0, 3000.0));

    #[test]
    fn auto_keeps_the_systems_scale_on_every_screen() {
        for native in [1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
            let p = plan(None, Some(native), BIG);
            assert_eq!((p.zoom, p.drawn, p.limited), (1.0, native, false), "{native}");
        }
    }

    #[test]
    fn a_fixed_size_is_the_same_size_whatever_the_systems_scale() {
        // Windows 100/125/150/175 %, Retina 2×, a bare X11 window manager's guessed 2×, a 3× screen.
        for native in [1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
            for size in SIZES {
                let p = plan(Some(size), Some(native), BIG);
                assert!((p.drawn - size as f32).abs() < 1e-5, "{size} on {native}: drawn {}", p.drawn);
                assert!(!p.limited, "{size} on {native}");
            }
        }
        // 100 % on a 2× screen really is half the Auto size: the zoom is 0.5.
        assert_eq!(plan(Some(1.0), Some(2.0), BIG).zoom, 0.5);
    }

    #[test]
    fn a_missing_or_unusable_scale_reads_as_1_and_never_gives_egui_a_bad_zoom() {
        for native in [None, Some(0.0), Some(-2.0), Some(f32::NAN), Some(f32::INFINITY)] {
            for choice in [None, Some(1.5), Some(f64::NAN), Some(f64::INFINITY), Some(-1.0), Some(0.0), Some(1e300)] {
                let p = plan(choice, native, Some((f32::NAN, 0.0)));
                assert!(p.zoom.is_finite() && (ZOOM_RANGE.0..=ZOOM_RANGE.1).contains(&p.zoom), "{choice:?} {native:?}: {p:?}");
                assert!(p.drawn.is_finite() && p.drawn > 0.0, "{choice:?} {native:?}: {p:?}");
            }
        }
        // A choice that isn't a usable number is Auto.
        assert_eq!(plan(Some(f64::NAN), Some(2.0), BIG), plan(None, Some(2.0), BIG));
    }

    #[test]
    fn a_small_window_holds_a_big_size_back_without_going_below_100() {
        // The minimum window on a Retina Mac: 800 × 500 points is 1600 × 1000 pixels.
        let min = Some((1600.0, 1000.0));
        let p = plan(Some(3.0), Some(2.0), min);
        assert_eq!((p.drawn, p.limited), (2.0, true));
        assert!(!plan(Some(2.0), Some(2.0), min).limited);
        // The same window on a 1× screen: 800 × 500 pixels hold only 100 %.
        let p = plan(Some(2.0), Some(1.0), Some((800.0, 500.0)));
        assert_eq!((p.drawn, p.limited), (1.0, true));
        // A window smaller than the minimum (a tiling window manager): never below 100 %, and never
        // below a smaller size the person chose.
        assert_eq!(plan(Some(2.0), Some(1.0), Some((400.0, 250.0))).drawn, 1.0);
        let p = plan(Some(0.75), Some(1.0), Some((400.0, 250.0)));
        assert_eq!((p.drawn, p.limited), (0.75, false));
    }

    #[test]
    fn the_limit_does_not_depend_on_the_zoom_so_it_cannot_flicker() {
        // The window's size in pixels is the same whatever zoom is drawn now, so the plan for it is too.
        let px = Some((1700.0, 1100.0));
        let a = plan(Some(2.5), Some(2.0), px);
        let b = plan(Some(2.5), Some(2.0), px);
        assert_eq!(a, b);
    }

    #[test]
    fn percentages_round_like_the_menu_shows_them() {
        assert_eq!([percent(0.75), percent(1.0), percent(1.25), percent(2.0), percent(1.7999)], [75, 100, 125, 200, 180]);
        assert_eq!([percent(f32::NAN), percent(-1.0)], [0, 0]);
    }
}
