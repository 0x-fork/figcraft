//! FigCraft text: fonts, shaping and layout.
//!
//! Fonts come from the optional craft-fonts build input (Inter and friends, embedded at build time)
//! and, on desktop, from the system's font folders. [`layout`] turns a text node's
//! [`TextData`](figcraft_doc::TextData) into positioned glyphs, lines and decorations; the renderer
//! fills [`Glyph::path`]s and the editor uses [`TextLayout::caret`] / [`TextLayout::hit`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod fontdb;
mod layout;

pub use fontdb::{CRAFT_FONTS, Face, FontDb, db, family_names, family_styles, find_face, scan_system_fonts};
pub use layout::{Decoration, Glyph, Line, TextLayout, layout};

use figcraft_doc::{Node, NodeKind, TextAlignV, TextAutoResize};

/// Lay out a text node at its current box, returning the layout and the vertical offset that
/// applies its vertical alignment.
pub fn layout_node(n: &Node) -> Option<(TextLayout, f64)> {
    let NodeKind::Text(t) = &n.kind else { return None };
    let l = layout(t, Some(n.width), Some(n.height));
    let dy = match (t.auto_resize, t.align_v) {
        (TextAutoResize::WidthAndHeight | TextAutoResize::Height, _) => 0.0,
        (_, TextAlignV::Top) => 0.0,
        (_, TextAlignV::Middle) => (n.height - l.height) / 2.0,
        (_, TextAlignV::Bottom) => n.height - l.height,
    };
    Some((l, dy))
}

/// The box size a text node should have after its content or style changed, per its resize mode.
pub fn auto_size(n: &Node) -> Option<(f64, f64)> {
    let NodeKind::Text(t) = &n.kind else { return None };
    match t.auto_resize {
        TextAutoResize::WidthAndHeight => {
            let l = layout(t, None, None);
            Some((l.width.ceil().max(1.0), l.height.max(1.0)))
        }
        TextAutoResize::Height => {
            let l = layout(t, Some(n.width), None);
            Some((n.width, l.height.max(1.0)))
        }
        TextAutoResize::None | TextAutoResize::Truncate => None,
    }
}

#[cfg(test)]
mod tests;
