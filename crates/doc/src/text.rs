//! Text node data: characters plus a base style and styled runs.

use figcraft_color::Paint;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit", content = "value", rename_all = "camelCase")]
pub enum LineHeight {
    Auto,
    Pixels(f64),
    Percent(f64),
}

impl Default for LineHeight {
    fn default() -> Self {
        LineHeight::Auto
    }
}

impl LineHeight {
    /// Resolved line height in px for `font_size` (`auto` uses the font's natural ratio).
    pub fn resolve(self, font_size: f64, natural_ratio: f64) -> f64 {
        match self {
            LineHeight::Auto => font_size * natural_ratio,
            LineHeight::Pixels(v) => v.max(0.0),
            LineHeight::Percent(p) => font_size * p.max(0.0) / 100.0,
        }
    }
    pub fn label(self) -> String {
        match self {
            LineHeight::Auto => "Auto".into(),
            LineHeight::Pixels(v) => trim_num(v),
            LineHeight::Percent(p) => format!("{}%", trim_num(p)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit", content = "value", rename_all = "camelCase")]
pub enum LetterSpacing {
    Pixels(f64),
    Percent(f64),
}

impl Default for LetterSpacing {
    fn default() -> Self {
        LetterSpacing::Percent(0.0)
    }
}

impl LetterSpacing {
    pub fn resolve(self, font_size: f64) -> f64 {
        match self {
            LetterSpacing::Pixels(v) => v,
            LetterSpacing::Percent(p) => font_size * p / 100.0,
        }
    }
    pub fn label(self) -> String {
        match self {
            LetterSpacing::Pixels(v) => trim_num(v),
            LetterSpacing::Percent(p) => format!("{}%", trim_num(p)),
        }
    }
}

pub fn trim_num(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract().abs() < 1e-9 { format!("{}", r as i64) } else { format!("{r}") }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
    Strikethrough,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TextCase {
    #[default]
    Original,
    Upper,
    Lower,
    Title,
    SmallCaps,
}

impl TextCase {
    pub fn apply(self, s: &str) -> String {
        match self {
            TextCase::Original | TextCase::SmallCaps => s.to_string(),
            TextCase::Upper => s.to_uppercase(),
            TextCase::Lower => s.to_lowercase(),
            TextCase::Title => {
                let mut out = String::with_capacity(s.len());
                let mut start = true;
                for ch in s.chars() {
                    if start && ch.is_alphabetic() {
                        out.extend(ch.to_uppercase());
                        start = false;
                    } else {
                        out.push(ch);
                        if ch.is_whitespace() {
                            start = true;
                        }
                    }
                }
                out
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TextAlignH {
    #[default]
    Left,
    Center,
    Right,
    Justified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TextAlignV {
    #[default]
    Top,
    Middle,
    Bottom,
}

/// How a text box resizes as its content changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TextAutoResize {
    /// Auto width: one line per paragraph, box grows both ways.
    #[default]
    WidthAndHeight,
    /// Fixed width, height grows.
    Height,
    /// Fixed size.
    None,
    /// Fixed size, truncate with an ellipsis.
    Truncate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ListStyle {
    #[default]
    None,
    Bulleted,
    Numbered,
}

/// Character style. In runs, every field is an override; `None` means "inherit the base".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    pub font_family: String,
    /// The style name, e.g. "Regular", "Bold", "Semi Bold Italic".
    pub font_style: String,
    pub font_size: f64,
    pub line_height: LineHeight,
    pub letter_spacing: LetterSpacing,
    pub decoration: TextDecoration,
    pub case: TextCase,
    /// Run fills (None = node fills).
    pub fills: Option<Vec<Paint>>,
    /// Hyperlink.
    pub link: Option<String>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: "Inter".into(),
            font_style: "Regular".into(),
            font_size: 12.0,
            line_height: LineHeight::Auto,
            letter_spacing: LetterSpacing::default(),
            decoration: TextDecoration::None,
            case: TextCase::Original,
            fills: None,
            link: None,
        }
    }
}

impl TextStyle {
    /// Numeric weight implied by the style name.
    pub fn weight(&self) -> u16 {
        style_weight(&self.font_style)
    }
    pub fn italic(&self) -> bool {
        self.font_style.to_ascii_lowercase().contains("italic")
    }
}

/// Weight from a style name ("Semi Bold Italic" → 600).
pub fn style_weight(style: &str) -> u16 {
    let s = style.to_ascii_lowercase().replace([' ', '-'], "");
    let table: [(&str, u16); 10] = [
        ("thin", 100),
        ("extralight", 200),
        ("ultralight", 200),
        ("light", 300),
        ("medium", 500),
        ("semibold", 600),
        ("demibold", 600),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("black", 900),
    ];
    for (k, w) in table {
        if s.contains(k) {
            return w;
        }
    }
    if s.contains("bold") {
        return 700;
    }
    if s.contains("heavy") {
        return 900;
    }
    400
}

/// A styled range of characters (char indices, end exclusive).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleRun {
    pub start: usize,
    pub end: usize,
    pub style: TextStyle,
}

/// Text node content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TextData {
    pub characters: String,
    pub style: TextStyle,
    /// Ranges whose style differs from `style`; full styles (already merged), non-overlapping, sorted.
    pub runs: Vec<StyleRun>,
    pub auto_resize: TextAutoResize,
    pub align_h: TextAlignH,
    pub align_v: TextAlignV,
    pub paragraph_spacing: f64,
    pub paragraph_indent: f64,
    pub list: ListStyle,
    /// Max lines when truncating (0 = as many as fit).
    pub max_lines: u32,
    /// OpenType feature toggles (e.g. "liga": false).
    pub features: std::collections::BTreeMap<String, bool>,
}

impl TextData {
    pub fn new(characters: impl Into<String>) -> Self {
        Self { characters: characters.into(), ..Default::default() }
    }

    pub fn char_count(&self) -> usize {
        self.characters.chars().count()
    }

    /// The style at char index `i`.
    pub fn style_at(&self, i: usize) -> &TextStyle {
        self.runs.iter().find(|r| r.start <= i && i < r.end).map(|r| &r.style).unwrap_or(&self.style)
    }

    /// Spans of uniform style covering the whole text: (start, end, style).
    pub fn spans(&self) -> Vec<(usize, usize, &TextStyle)> {
        let n = self.char_count();
        let mut out = Vec::new();
        let mut pos = 0;
        let mut runs: Vec<&StyleRun> = self.runs.iter().filter(|r| r.start < r.end && r.start < n).collect();
        runs.sort_by_key(|r| r.start);
        for r in runs {
            let start = r.start.max(pos);
            let end = r.end.min(n);
            if start >= end {
                continue;
            }
            if start > pos {
                out.push((pos, start, &self.style));
            }
            out.push((start, end, &r.style));
            pos = end;
        }
        if pos < n || out.is_empty() {
            out.push((pos, n, &self.style));
        }
        out
    }

    /// Apply `f` to the style of chars `start..end` (all text when the range is the whole text or
    /// empty text), splitting runs as needed and merging equal neighbours.
    pub fn restyle(&mut self, start: usize, end: usize, f: impl Fn(&mut TextStyle)) {
        let n = self.char_count();
        let (start, end) = (start.min(n), end.min(n));
        if n == 0 || (start == 0 && end >= n) {
            f(&mut self.style);
            for r in &mut self.runs {
                f(&mut r.style);
            }
            self.normalize();
            return;
        }
        if start >= end {
            return;
        }
        // Expand to per-char styles, modify, re-compress.
        let mut per: Vec<TextStyle> = (0..n).map(|i| self.style_at(i).clone()).collect();
        for s in per.iter_mut().take(end).skip(start) {
            f(s);
        }
        self.runs.clear();
        let mut i = 0;
        while i < n {
            let Some(s) = per.get(i) else { break };
            let mut j = i + 1;
            while per.get(j).is_some_and(|t| t == s) {
                j += 1;
            }
            if *s != self.style {
                self.runs.push(StyleRun { start: i, end: j, style: s.clone() });
            }
            i = j;
        }
    }

    /// Drop empty/out-of-range runs and runs equal to the base style.
    pub fn normalize(&mut self) {
        let n = self.char_count();
        let base = self.style.clone();
        self.runs.retain(|r| r.start < r.end && r.start < n && r.style != base);
        for r in &mut self.runs {
            r.end = r.end.min(n);
        }
        self.runs.sort_by_key(|r| r.start);
    }

    /// Replace chars `start..end` with `text`, shifting runs (inserted text takes the style at `start`).
    pub fn replace_range(&mut self, start: usize, end: usize, text: &str) {
        let n = self.char_count();
        let (start, end) = (start.min(n), end.min(n).max(start.min(n)));
        let byte = |i: usize| self.characters.char_indices().nth(i).map(|(b, _)| b).unwrap_or(self.characters.len());
        let (bs, be) = (byte(start), byte(end));
        let mut s = String::with_capacity(self.characters.len() + text.len());
        s.push_str(self.characters.get(..bs).unwrap_or(""));
        s.push_str(text);
        s.push_str(self.characters.get(be..).unwrap_or(""));
        self.characters = s;
        let inserted = text.chars().count();
        let removed = end - start;
        for r in &mut self.runs {
            let shift = |p: usize| -> usize {
                if p <= start {
                    p
                } else if p >= end {
                    p - removed + inserted
                } else {
                    start + inserted
                }
            };
            // A run touching the insertion point at its end grows with the typed text.
            let new_start = shift(r.start);
            let new_end = if r.end == start && r.start < start { start + inserted } else { shift(r.end) };
            r.start = new_start;
            r.end = new_end;
        }
        self.normalize();
    }

    /// Whether every char shares one style (the panel shows "Mixed" otherwise).
    pub fn is_uniform(&self) -> bool {
        self.runs.is_empty()
    }
}
