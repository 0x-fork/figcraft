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
        direction: Direction,
    },
    MoveOut {
        direction: Direction,
    },
    Push {
        direction: Direction,
    },
    SlideIn {
        direction: Direction,
    },
    SlideOut {
        direction: Direction,
    },
}

impl Transition {
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
        #[serde(default)]
        close_on_outside: bool,
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
    #[serde(default = "default_duration")]
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
