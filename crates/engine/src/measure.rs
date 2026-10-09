//! Text measurement for the layout engine.

use figcraft_doc::{Document, NodeId, NodeKind, TextAutoResize};

pub struct TextMeasure;

impl figcraft_layout::Measure for TextMeasure {
    fn text_height(&self, doc: &Document, id: NodeId, width: f64) -> Option<f64> {
        let n = doc.node(id)?;
        let NodeKind::Text(t) = &n.kind else { return None };
        let mut t = t.clone();
        if t.auto_resize == TextAutoResize::WidthAndHeight {
            t.auto_resize = TextAutoResize::Height;
        }
        Some(figcraft_text::layout(&t, Some(width), None).height.max(1.0))
    }
    fn text_size(&self, doc: &Document, id: NodeId) -> Option<(f64, f64)> {
        let n = doc.node(id)?;
        figcraft_text::auto_size(n)
    }
}
