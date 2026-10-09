//! Prototype data: interactions (trigger → actions) with transitions, flows and overflow.

use crate::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[derive(Default)]
pub enum Trigger {
    #[default]
    OnClick,
    OnHover,
    OnPress,
    OnDrag,
    MouseEnter,
    MouseLeave,
    MouseDown,
    MouseUp,
    AfterDelay {
        ms: u32,
    },
    KeyPress {
        key: String,
    },
}

impl Trigger {
    pub fn label(&self) -> String {
        match self {
            Trigger::OnClick => "On click".into(),
            Trigger::OnHover => "While hovering".into(),
            Trigger::OnPress => "While pressing".into(),
            Trigger::OnDrag => "On drag".into(),
            Trigger::MouseEnter => "Mouse enter".into(),
            Trigger::MouseLeave => "Mouse leave".into(),
            Trigger::MouseDown => "Mouse down".into(),
            Trigger::MouseUp => "Mouse up".into(),
            Trigger::AfterDelay { ms } => format!("After delay {ms}ms"),
            Trigger::KeyPress { key } => format!("Key/Gamepad {key}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Easing {
    Linear,
    EaseIn,
    #[default]
    EaseOut,
    EaseInOut,
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,
    Gentle,
    Quick,
    Bouncy,
    Slow,
}

impl Easing {
    pub const ALL: [Easing; 11] = [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::EaseInBack,
        Easing::EaseOutBack,
        Easing::EaseInOutBack,
        Easing::Gentle,
        Easing::Quick,
        Easing::Bouncy,
        Easing::Slow,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Easing::Linear => "Linear",
            Easing::EaseIn => "Ease in",
            Easing::EaseOut => "Ease out",
            Easing::EaseInOut => "Ease in and out",
            Easing::EaseInBack => "Ease in back",
            Easing::EaseOutBack => "Ease out back",
            Easing::EaseInOutBack => "Ease in and out back",
            Easing::Gentle => "Gentle",
            Easing::Quick => "Quick",
            Easing::Bouncy => "Bouncy",
            Easing::Slow => "Slow",
        }
    }

    /// Spring presets run longer than the chosen duration in the incumbent; we keep the duration.
    pub fn is_spring(self) -> bool {
        matches!(self, Easing::Gentle | Easing::Quick | Easing::Bouncy | Easing::Slow)
    }

    /// Evaluate the curve at `t` (0..1).
    pub fn eval(self, t: f64) -> f64 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
        let back = 1.70158;
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t * t,
            Easing::EaseOut => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Easing::EaseInBack => (back + 1.0) * t * t * t - back * t * t,
            Easing::EaseOutBack => 1.0 + (back + 1.0) * (t - 1.0).powi(3) + back * (t - 1.0).powi(2),
            Easing::EaseInOutBack => {
                let c2 = back * 1.525;
                if t < 0.5 {
                    ((2.0 * t).powi(2) * ((c2 + 1.0) * 2.0 * t - c2)) / 2.0
                } else {
                    ((2.0 * t - 2.0).powi(2) * ((c2 + 1.0) * (t * 2.0 - 2.0) + c2) + 2.0) / 2.0
                }
            }
            // Spring presets approximated by damped oscillations.
            Easing::Gentle => spring(t, 6.0, 0.7),
            Easing::Quick => spring(t, 10.0, 0.8),
            Easing::Bouncy => spring(t, 9.0, 0.35),
            Easing::Slow => spring(t, 4.0, 0.9),
        }
    }
}

fn spring(t: f64, freq: f64, damping: f64) -> f64 {
    if t >= 1.0 {
        return 1.0;
    }
    let decay = (-damping * 8.0 * t).exp();
    1.0 - decay * (freq * t * std::f64::consts::PI).cos() * (1.0 - t)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[derive(Default)]
pub enum Transition {
    #[default]
    Instant,
    Dissolve,
    SmartAnimate,
    MoveIn {
        #[serde(default)]
        direction: Direction,
    },
    MoveOut {
        #[serde(default)]
        direction: Direction,
    },
    Push {
        #[serde(default)]
        direction: Direction,
    },
    SlideIn {
        #[serde(default)]
        direction: Direction,
    },
    SlideOut {
        #[serde(default)]
        direction: Direction,
    },
}

impl Transition {
    /// The transition's direction, if it has one.
    pub fn direction(&self) -> Option<Direction> {
        match self {
            Transition::MoveIn { direction }
            | Transition::MoveOut { direction }
            | Transition::Push { direction }
            | Transition::SlideIn { direction }
            | Transition::SlideOut { direction } => Some(*direction),
            _ => None,
        }
    }

    /// The same transition with another direction (no-op for directionless ones).
    pub fn with_direction(self, d: Direction) -> Transition {
        match self {
            Transition::MoveIn { .. } => Transition::MoveIn { direction: d },
            Transition::MoveOut { .. } => Transition::MoveOut { direction: d },
            Transition::Push { .. } => Transition::Push { direction: d },
            Transition::SlideIn { .. } => Transition::SlideIn { direction: d },
            Transition::SlideOut { .. } => Transition::SlideOut { direction: d },
            other => other,
        }
    }

    /// The opposite motion (used when a "while hovering/pressing" state reverts).
    pub fn reversed(self) -> Transition {
        let opp = |d: Direction| match d {
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
            Direction::Top => Direction::Bottom,
            Direction::Bottom => Direction::Top,
        };
        match self {
            Transition::MoveIn { direction } => Transition::MoveOut { direction: opp(direction) },
            Transition::MoveOut { direction } => Transition::MoveIn { direction: opp(direction) },
            Transition::Push { direction } => Transition::Push { direction: opp(direction) },
            Transition::SlideIn { direction } => Transition::SlideOut { direction: opp(direction) },
            Transition::SlideOut { direction } => Transition::SlideIn { direction: opp(direction) },
            other => other,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Transition::Instant => "Instant",
            Transition::Dissolve => "Dissolve",
            Transition::SmartAnimate => "Smart animate",
            Transition::MoveIn { .. } => "Move in",
            Transition::MoveOut { .. } => "Move out",
            Transition::Push { .. } => "Push",
            Transition::SlideIn { .. } => "Slide in",
            Transition::SlideOut { .. } => "Slide out",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OverlayPosition {
    #[default]
    Center,
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    Manual,
}

impl OverlayPosition {
    pub const ALL: [OverlayPosition; 8] = [
        OverlayPosition::Center,
        OverlayPosition::TopLeft,
        OverlayPosition::TopCenter,
        OverlayPosition::TopRight,
        OverlayPosition::BottomLeft,
        OverlayPosition::BottomCenter,
        OverlayPosition::BottomRight,
        OverlayPosition::Manual,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OverlayPosition::Center => "Center",
            OverlayPosition::TopLeft => "Top left",
            OverlayPosition::TopCenter => "Top center",
            OverlayPosition::TopRight => "Top right",
            OverlayPosition::BottomLeft => "Bottom left",
            OverlayPosition::BottomCenter => "Bottom center",
            OverlayPosition::BottomRight => "Bottom right",
            OverlayPosition::Manual => "Manual",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    Navigate {
        dest: NodeId,
    },
    Back,
    ScrollTo {
        dest: NodeId,
    },
    OpenOverlay {
        dest: NodeId,
        #[serde(default)]
        position: OverlayPosition,
        #[serde(default, alias = "closeOnOutside")]
        close_on_outside: bool,
        /// Colour drawn behind the overlay (over the frame underneath); None = no background.
        #[serde(default)]
        background: Option<figcraft_color::Color>,
    },
    SwapOverlay {
        dest: NodeId,
    },
    CloseOverlay,
    OpenUrl {
        url: String,
    },
    /// Change an instance to another variant (interactive components).
    ChangeTo {
        dest: NodeId,
    },
    SetVariable {
        variable: String,
        value: serde_json::Value,
    },
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::Navigate { .. } => "Navigate to",
            Action::Back => "Back",
            Action::ScrollTo { .. } => "Scroll to",
            Action::OpenOverlay { .. } => "Open overlay",
            Action::SwapOverlay { .. } => "Swap overlay",
            Action::CloseOverlay => "Close overlay",
            Action::OpenUrl { .. } => "Open link",
            Action::ChangeTo { .. } => "Change to",
            Action::SetVariable { .. } => "Set variable",
        }
    }
    pub fn dest(&self) -> Option<NodeId> {
        match self {
            Action::Navigate { dest }
            | Action::ScrollTo { dest }
            | Action::OpenOverlay { dest, .. }
            | Action::SwapOverlay { dest }
            | Action::ChangeTo { dest } => Some(*dest),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Interaction {
    #[serde(default)]
    pub trigger: Trigger,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub transition: Transition,
    #[serde(default = "default_duration", alias = "durationMs")]
    pub duration_ms: u32,
    #[serde(default)]
    pub easing: Easing,
}

fn default_duration() -> u32 {
    300
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Overflow {
    #[default]
    None,
    Horizontal,
    Vertical,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ScrollPosition {
    #[default]
    Scrolls,
    Fixed,
    Sticky,
}

/// A prototype flow's starting point on a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    pub name: String,
    pub start: NodeId,
    #[serde(default)]
    pub description: String,
}

/// Prototype device settings on a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrototypeSettings {
    pub device: String,
    pub background: figcraft_color::Color,
}

impl Default for PrototypeSettings {
    fn default() -> Self {
        Self { device: "None".into(), background: figcraft_color::Color::from_rgb8_const(0x1E, 0x1E, 0x1E) }
    }
}
