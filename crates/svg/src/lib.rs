//! SVG export and import.
//!
//! Export writes one `<svg>` sized to the exported nodes' bounds. Shapes become `<path>`s with
//! their fills (solid, linear/radial gradients; angular and diamond fall back to their first
//! colour... or a raster pattern), strokes are written as filled outlines so Inside/Outside
//! alignment survives exactly, text is outlined, images are embedded as data URIs, and shadows
//! and blurs become SVG filters.
//!
//! Import (via usvg) turns an SVG file into a frame of vector nodes.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod import;
pub use import::{import_svg, ImportError};

use std::fmt::Write as _;

use base64::Engine as _;
use figcraft_color::{Color, GradientKind, Paint, PaintKind};
use figcraft_doc::{Document, EffectKind, Node, NodeId, NodeKind};
use figcraft_geom::{Affine, BezPath, Rect};

#[derive(Clone, Debug)]
pub struct SvgOptions {
    /// Include node ids as `id` attributes (Figma's "Include id attribute").
    pub include_ids: bool,
    /// Outline text (always true for now: no font embedding).
    pub outline_text: bool,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self { include_ids: false, outline_text: true }
    }
}

struct W<'a> {
    doc: &'a Document,
    defs: String,
    body: String,
    next: u32,
    opts: &'a SvgOptions,
}

fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let r = (v * 1000.0).round() / 1000.0;
    let s = format!("{r}");
    if s == "-0" { "0".into() } else { s }
}

fn path_d(p: &BezPath) -> String {
    use figcraft_geom::kurbo::PathEl::*;
    let mut s = String::new();
    for el in p.elements() {
        match *el {
            MoveTo(a) => {
                let _ = write!(s, "M{} {}", num(a.x), num(a.y));
            }
            LineTo(a) => {
                let _ = write!(s, "L{} {}", num(a.x), num(a.y));
            }
            QuadTo(a, b) => {
                let _ = write!(s, "Q{} {} {} {}", num(a.x), num(a.y), num(b.x), num(b.y));
            }
            CurveTo(a, b, c) => {
                let _ = write!(s, "C{} {} {} {} {} {}", num(a.x), num(a.y), num(b.x), num(b.y), num(c.x), num(c.y));
            }
            ClosePath => s.push('Z'),
        }
    }
    s
}

fn matrix(a: Affine) -> String {
    let c = a.as_coeffs();
    format!("matrix({} {} {} {} {} {})", num(c[0]), num(c[1]), num(c[2]), num(c[3]), num(c[4]), num(c[5]))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hex(c: Color) -> String {
    format!("#{}", c.to_hex())
}

impl W<'_> {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    /// `fill="..." [fill-opacity]` for a paint on a w×h box; `None` when it paints nothing.
    fn paint_attr(&mut self, p: &Paint, w: f64, h: f64) -> Option<String> {
        if !p.visible || p.opacity <= 0.0 {
            return None;
        }
        let blend = if matches!(p.blend, figcraft_color::BlendMode::Normal | figcraft_color::BlendMode::PassThrough) {
            String::new()
        } else {
            format!(" style=\"mix-blend-mode:{}\"", p.blend.css())
        };
        match &p.kind {
            PaintKind::Solid { color } => {
                let a = color.a * p.opacity;
                let op = if a < 0.999 { format!(" fill-opacity=\"{}\"", num(f64::from(a))) } else { String::new() };
                Some(format!("fill=\"{}\"{op}{blend}", hex(*color)))
            }
            PaintKind::Gradient(g) => {
                let gid = self.id("paint");
                let stops: String = g
                    .sorted_stops()
                    .iter()
                    .map(|s| {
                        let a = s.color.a * p.opacity;
                        let op = if a < 0.999 { format!(" stop-opacity=\"{}\"", num(f64::from(a))) } else { String::new() };
                        format!("<stop offset=\"{}\" stop-color=\"{}\"{op}/>", num(f64::from(s.position)), hex(s.color))
                    })
                    .collect();
                let (f, t, k) = ((g.from.0 * w, g.from.1 * h), (g.to.0 * w, g.to.1 * h), (g.width.0 * w, g.width.1 * h));
                match g.kind {
                    GradientKind::Linear => {
                        let _ = write!(
                            self.defs,
                            "<linearGradient id=\"{gid}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" gradientUnits=\"userSpaceOnUse\">{stops}</linearGradient>",
                            num(f.0),
                            num(f.1),
                            num(t.0),
                            num(t.1)
                        );
                    }
                    _ => {
                        // Unit circle mapped onto the handle ellipse.
                        let m = Affine::new([t.0 - f.0, t.1 - f.1, k.0 - f.0, k.1 - f.1, f.0, f.1]);
                        let _ = write!(
                            self.defs,
                            "<radialGradient id=\"{gid}\" cx=\"0\" cy=\"0\" r=\"1\" gradientUnits=\"userSpaceOnUse\" gradientTransform=\"{}\">{stops}</radialGradient>",
                            matrix(m)
                        );
                    }
                }
                Some(format!("fill=\"url(#{gid})\"{blend}"))
            }
            PaintKind::Image(img) => {
                let data = self.doc.images.get(&img.image)?;
                let pid = self.id("pattern");
                let iid = self.id("image");
                let mime = sniff_mime(&data.bytes);
                let b64 = base64::engine::general_purpose::STANDARD.encode(data.bytes.as_slice());
                let t = figcraft_render::paint::image_transform(img, f64::from(data.width), f64::from(data.height), w, h);
                let m = Affine::new([f64::from(t.sx), f64::from(t.ky), f64::from(t.kx), f64::from(t.sy), f64::from(t.tx), f64::from(t.ty)]);
                let (pw, ph) = if img.scale_mode == figcraft_color::ImageScaleMode::Tile {
                    (f64::from(data.width), f64::from(data.height))
                } else {
                    (f64::from(data.width) * 4.0, f64::from(data.height) * 4.0)
                };
                let _ = write!(
                    self.defs,
                    "<pattern id=\"{pid}\" patternUnits=\"userSpaceOnUse\" width=\"{}\" height=\"{}\" patternTransform=\"{}\"><image id=\"{iid}\" width=\"{}\" height=\"{}\" href=\"data:{mime};base64,{b64}\"/></pattern>",
                    num(pw),
                    num(ph),
                    matrix(m),
                    data.width,
                    data.height
                );
                let op = if p.opacity < 0.999 { format!(" fill-opacity=\"{}\"", num(f64::from(p.opacity))) } else { String::new() };
                Some(format!("fill=\"url(#{pid})\"{op}{blend}"))
            }
        }
    }

    fn filter_for(&mut self, n: &Node) -> Option<String> {
        let fx: Vec<&figcraft_doc::Effect> = n.effects.iter().filter(|e| e.visible).collect();
        if fx.is_empty() {
            return None;
        }
        let fid = self.id("filter");
        let mut f = format!("<filter id=\"{fid}\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\" color-interpolation-filters=\"sRGB\">");
        let mut merge = Vec::new();
        for (i, e) in fx.iter().enumerate() {
            match &e.kind {
                EffectKind::DropShadow(s) => {
                    let _ = write!(
                        f,
                        "<feMorphology in=\"SourceAlpha\" operator=\"{}\" radius=\"{}\" result=\"s{i}m\"/><feOffset in=\"s{i}m\" dx=\"{}\" dy=\"{}\"/><feGaussianBlur stdDeviation=\"{}\"/><feColorMatrix type=\"matrix\" values=\"0 0 0 0 {} 0 0 0 0 {} 0 0 0 0 {} 0 0 0 {} 0\" result=\"s{i}\"/>",
                        if s.spread >= 0.0 { "dilate" } else { "erode" },
                        num(s.spread.abs()),
                        num(s.offset.0),
                        num(s.offset.1),
                        num(s.radius / 2.0),
                        num(f64::from(s.color.r)),
                        num(f64::from(s.color.g)),
                        num(f64::from(s.color.b)),
                        num(f64::from(s.color.a))
                    );
                    merge.push(format!("s{i}"));
                }
                EffectKind::LayerBlur { radius } => {
                    let _ = write!(f, "<feGaussianBlur in=\"SourceGraphic\" stdDeviation=\"{}\" result=\"blur\"/>", num(radius / 2.0));
                }
                _ => {}
            }
        }
        let has_blur = fx.iter().any(|e| matches!(e.kind, EffectKind::LayerBlur { .. }));
        f.push_str("<feMerge>");
        for m in &merge {
            let _ = write!(f, "<feMergeNode in=\"{m}\"/>");
        }
        let _ = write!(f, "<feMergeNode in=\"{}\"/></feMerge></filter>", if has_blur { "blur" } else { "SourceGraphic" });
        self.defs.push_str(&f);
        Some(fid)
    }

    fn node(&mut self, id: NodeId, xf: Affine, depth: usize) {
        if depth > 256 {
            return;
        }
        let Some(n) = self.doc.node(id) else { return };
        if !n.visible || n.is_mask {
            return;
        }
        let xf = xf * n.transform;
        let mut attrs = String::new();
        if self.opts.include_ids {
            let _ = write!(attrs, " id=\"{}\"", esc(&n.name.replace(' ', "_")));
        }
        if n.opacity < 0.999 {
            let _ = write!(attrs, " opacity=\"{}\"", num(f64::from(n.opacity)));
        }
        if !matches!(n.blend, figcraft_color::BlendMode::Normal | figcraft_color::BlendMode::PassThrough) {
            let _ = write!(attrs, " style=\"mix-blend-mode:{}\"", n.blend.css());
        }
        if let Some(fid) = self.filter_for(n) {
            let _ = write!(attrs, " filter=\"url(#{fid})\"");
        }
        let _ = write!(self.body, "<g{attrs}>");
        let (w, h) = (n.width, n.height);
        let outline = match &n.kind {
            NodeKind::Group | NodeKind::Page { .. } => None,
            _ => figcraft_render::node_outline(self.doc, n),
        };
        let closed = figcraft_doc::geometry::is_closed(n) || matches!(n.kind, NodeKind::BooleanOp { .. } | NodeKind::Text(_));
        if let Some((path, rule)) = &outline
            && closed
        {
            let fr = if *rule == figcraft_render::FillRule::EvenOdd { " fill-rule=\"evenodd\"" } else { "" };
            for p in &n.fills {
                if let Some(a) = self.paint_attr(p, w, h) {
                    let _ = write!(self.body, "<path transform=\"{}\" d=\"{}\" {a}{fr}/>", matrix(xf), path_d(path));
                }
            }
        }
        if !matches!(n.kind, NodeKind::BooleanOp { .. }) && !n.children.is_empty() {
            let clip = n.clip && n.kind.is_frame_like();
            if clip && let Some((p, _)) = &outline {
                let cid = self.id("clip");
                let mut cp = p.clone();
                cp.apply_affine(xf);
                let _ = write!(self.defs, "<clipPath id=\"{cid}\"><path d=\"{}\"/></clipPath>", path_d(&cp));
                let _ = write!(self.body, "<g clip-path=\"url(#{cid})\">");
            }
            let kids = n.children.clone();
            let mut i = 0;
            while i < kids.len() {
                let Some(&c) = kids.get(i) else { break };
                if self.doc.node(c).is_some_and(|cn| cn.is_mask && cn.visible) {
                    let mid = self.id("mask");
                    let saved = std::mem::take(&mut self.body);
                    if let Some(mn) = self.doc.node(c) {
                        let mxf = xf * mn.transform;
                        if let Some((mp, _)) = figcraft_render::node_outline(self.doc, mn) {
                            let _ = write!(self.body, "<path transform=\"{}\" d=\"{}\" fill=\"white\"/>", matrix(mxf), path_d(&mp));
                        }
                    }
                    let mask_body = std::mem::replace(&mut self.body, saved);
                    let _ = write!(self.defs, "<mask id=\"{mid}\" maskUnits=\"userSpaceOnUse\">{mask_body}</mask>");
                    let _ = write!(self.body, "<g mask=\"url(#{mid})\">");
                    let mut j = i + 1;
                    while let Some(&k) = kids.get(j) {
                        if self.doc.node(k).is_some_and(|kn| kn.is_mask) {
                            break;
                        }
                        self.node(k, xf, depth + 1);
                        j += 1;
                    }
                    self.body.push_str("</g>");
                    i = j;
                    continue;
                }
                self.node(c, xf, depth + 1);
                i += 1;
            }
            if clip && outline.is_some() {
                self.body.push_str("</g>");
            }
        }
        if n.has_visible_stroke() {
            let geom = match &n.kind {
                NodeKind::BooleanOp { .. } | NodeKind::Text(_) => outline.as_ref().map(|(p, _)| p.clone()),
                _ => figcraft_doc::geometry::stroke_geometry(n),
            };
            if let Some(g) = geom {
                let area = figcraft_render::draw::stroke_area(n, &g, closed);
                let even = n.stroke.sides.is_some() || (n.stroke.dashes.is_empty() && closed && n.stroke.align != figcraft_doc::StrokeAlign::Center);
                let fr = if even { " fill-rule=\"evenodd\"" } else { "" };
                for p in &n.strokes {
                    if let Some(a) = self.paint_attr(p, w, h) {
                        let _ = write!(self.body, "<path transform=\"{}\" d=\"{}\" {a}{fr}/>", matrix(xf), path_d(&area));
                    }
                }
            }
        }
        self.body.push_str("</g>");
    }
}

fn sniff_mime(b: &[u8]) -> &'static str {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if b.starts_with(b"GIF") {
        "image/gif"
    } else if b.len() > 12 && b.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// Export `ids` (in z-order) as one SVG document covering their export bounds.
pub fn export_svg(doc: &Document, ids: &[NodeId], opts: &SvgOptions) -> Option<String> {
    let bounds: Rect = ids.iter().filter_map(|i| figcraft_render::export_bounds(doc, *i)).reduce(|a, b| a.union(b))?;
    let mut w = W { doc, defs: String::new(), body: String::new(), next: 0, opts };
    let origin = Affine::translate((-bounds.x0, -bounds.y0));
    for id in ids {
        let parent_xf = doc.node(*id).and_then(|n| n.parent).map(|p| doc.world_transform(p)).unwrap_or(Affine::IDENTITY);
        w.node(*id, origin * parent_xf, 0);
    }
    let (bw, bh) = (bounds.width().max(0.0), bounds.height().max(0.0));
    let defs = if w.defs.is_empty() { String::new() } else { format!("<defs>{}</defs>", w.defs) };
    Some(format!(
        "<svg width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\">{}{}</svg>\n",
        num(bw),
        num(bh),
        num(bw),
        num(bh),
        w.body,
        defs
    ))
}

#[cfg(test)]
mod tests;
