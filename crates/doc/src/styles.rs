//! Shared styles and variables.

use crate::props::{Effect, LayoutGrid};
use crate::text::TextStyle;
use figcraft_color::{Color, Paint};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type StyleId = String;
pub type VariableId = String;
pub type CollectionId = String;
pub type ModeId = String;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StyleKind {
    Paint { paints: Vec<Paint> },
    Text { style: TextStyle },
    Effect { effects: Vec<Effect> },
    Grid { grids: Vec<LayoutGrid> },
}

impl StyleKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            StyleKind::Paint { .. } => "paint",
            StyleKind::Text { .. } => "text",
            StyleKind::Effect { .. } => "effect",
            StyleKind::Grid { .. } => "grid",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub id: StyleId,
    /// Slash-separated name: "Brand/Primary".
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    pub kind: StyleKind,
}

/// A node's links to shared styles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StyleRefs {
    pub fill: Option<StyleId>,
    pub stroke: Option<StyleId>,
    pub effect: Option<StyleId>,
    pub grid: Option<StyleId>,
    pub text: Option<StyleId>,
}

impl StyleRefs {
    pub fn is_empty(&self) -> bool {
        *self == StyleRefs::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VariableType {
    Color,
    Number,
    String,
    Boolean,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum VariableValue {
    Color(Color),
    Number(f64),
    String(String),
    Boolean(bool),
    Alias(VariableId),
}

impl VariableValue {
    pub fn type_of(&self) -> Option<VariableType> {
        match self {
            VariableValue::Color(_) => Some(VariableType::Color),
            VariableValue::Number(_) => Some(VariableType::Number),
            VariableValue::String(_) => Some(VariableType::String),
            VariableValue::Boolean(_) => Some(VariableType::Boolean),
            VariableValue::Alias(_) => None,
        }
    }
    pub fn display(&self) -> String {
        match self {
            VariableValue::Color(c) => c.to_hex(),
            VariableValue::Number(n) => crate::text::trim_num(*n),
            VariableValue::String(s) => s.clone(),
            VariableValue::Boolean(b) => b.to_string(),
            VariableValue::Alias(a) => format!("→ {a}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mode {
    pub id: ModeId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub id: CollectionId,
    pub name: String,
    pub modes: Vec<Mode>,
}

impl Collection {
    pub fn default_mode(&self) -> Option<&ModeId> {
        self.modes.first().map(|m| &m.id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub id: VariableId,
    pub name: String,
    pub collection: CollectionId,
    #[serde(rename = "resolvedType")]
    pub kind: VariableType,
    /// Value per mode id.
    pub values: BTreeMap<ModeId, VariableValue>,
    #[serde(default)]
    pub description: String,
}

/// All variables of a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Variables {
    pub collections: Vec<Collection>,
    pub variables: Vec<Variable>,
}

impl Variables {
    pub fn get(&self, id: &str) -> Option<&Variable> {
        self.variables.iter().find(|v| v.id == id || v.name == id)
    }

    pub fn collection(&self, id: &str) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id || c.name == id)
    }

    /// Resolve `id` in `modes` (collection → chosen mode; default mode otherwise), following aliases
    /// (bounded, cycles return None).
    pub fn resolve(&self, id: &str, modes: &BTreeMap<CollectionId, ModeId>) -> Option<VariableValue> {
        let mut cur = id.to_string();
        for _ in 0..32 {
            let var = self.get(&cur)?;
            let coll = self.collection(&var.collection)?;
            let mode = modes.get(&coll.id).filter(|m| var.values.contains_key(*m)).or_else(|| coll.default_mode())?;
            match var.values.get(mode).or_else(|| var.values.values().next())? {
                VariableValue::Alias(next) => cur = next.clone(),
                v => return Some(v.clone()),
            }
        }
        None
    }
}
