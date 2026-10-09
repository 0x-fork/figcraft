//! SVG import: usvg's normalised tree → a frame of vector nodes (groups kept as groups).

use figcraft_color::{Color, ColorStop, Gradient, GradientKind, Paint};
use figcraft_doc::{Document, Node, NodeId, NodeKind, StrokeAlign, StrokeCap, StrokeJoin, VectorNetwork, WindingRule};
use figcraft_geom::{Affine, BezPath, Point, Rect, Shape};

#[derive(Debug)]
pub struct ImportError(pub String);

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SVG import failed: {}", self.0)
    }
}

fn xf(t: usvg::Transform) -> Affine {
    Affine::new([f64::from(t.sx), f64::from(t.ky), f64::from(t.kx), f64::from(t.sy), f64::from(t.tx), f64::from(t.ty)])
}

fn to_bez(p: &usvg::tiny_skia_path::Path) -> BezPath {
    use usvg::tiny_skia_path::PathSegment::*;
    let mut b = BezPath::new();
    for s in p.segments() {
        match s {
            MoveTo(a) => b.move_to((f64::from(a.x), f64::from(a.y))),
            LineTo(a) => b.line_to((f64::from(a.x), f64::from(a.y))),
            QuadTo(c, a) => b.quad_to((f64::from(c.x), f64::from(c.y)), (f64::from(a.x), f64::from(a.y))),
            CubicTo(c1, c2, a) => b.curve_to((f64::from(c1.x), f64::from(c1.y)), (f64::from(c2.x), f64::from(c2.y)), (f64::from(a.x), f64::from(a.y))),
            Close => b.close_path(),
        }
    }
    b
}

fn color(c: usvg::Color, op: f32) -> Color {
    Color::new(f32::from(c.red) / 255.0, f32::from(c.green) / 255.0, f32::from(c.blue) / 255.0, op)
}

/// Convert a usvg paint into ours for a node whose local box is `bbox` (absolute coordinates) and
/// whose absolute transform is `abs`.
fn paint(p: &usvg::Paint, opacity: f32, bbox: Rect, abs: Affine) -> Option<Paint> {
    let to_unit = |pt: Point| -> (f64, f64) {
        let w = bbox.width().max(1e-9);
        let h = bbox.height().max(1e-9);
        ((pt.x - bbox.x0) / w, (pt.y - bbox.y0) / h)
    };
    match p {
        usvg::Paint::Color(c) => Some(Paint::solid(color(*c, 1.0)).with_opacity(opacity)),
        usvg::Paint::LinearGradient(g) => {
            let gt = abs * xf(g.transform());
            let a = gt * Point::new(f64::from(g.x1()), f64::from(g.y1()));
            let b = gt * Point::new(f64::from(g.x2()), f64::from(g.y2()));
            let stops = g.stops().iter().map(|s| ColorStop { position: s.offset().get(), color: color(s.color(), s.opacity().get()) }).collect();
            let from = to_unit(a);
            let to = to_unit(b);
            Some(Paint::gradient(Gradient { kind: GradientKind::Linear, stops, from, to, width: (from.0 - (to.1 - from.1), from.1 + (to.0 - from.0)) }).with_opacity(opacity))
        }
        usvg::Paint::RadialGradient(g) => {
            let gt = abs * xf(g.transform());
            let c = gt * Point::new(f64::from(g.cx()), f64::from(g.cy()));
            let r = f64::from(g.r().get());
            let ex = gt * Point::new(f64::from(g.cx()) + r, f64::from(g.cy()));
            let ey = gt * Point::new(f64::from(g.cx()), f64::from(g.cy()) + r);
            let stops = g.stops().iter().map(|s| ColorStop { position: s.offset().get(), color: color(s.color(), s.opacity().get()) }).collect();
            Some(Paint::gradient(Gradient { kind: GradientKind::Radial, stops, from: to_unit(c), to: to_unit(ex), width: to_unit(ey) }).with_opacity(opacity))
        }
        usvg::Paint::Pattern(_) => None,
    }
}

trait WithOpacity {
    fn with_opacity(self, o: f32) -> Self;
}

impl WithOpacity for Paint {
    fn with_opacity(mut self, o: f32) -> Self {
        self.opacity *= o.clamp(0.0, 1.0);
        self
    }
}

struct Ctx<'a> {
    doc: &'a mut Document,
    /// Inverse of the root frame's placement: absolute SVG → frame-local.
    count: usize,
}

const MAX_NODES: usize = 100_000;

fn add_group(cx: &mut Ctx, g: &usvg::Group, parent: NodeId, parent_abs: Affine, depth: usize) {
    if depth > 128 {
        return;
    }
    for child in g.children() {
        if cx.count > MAX_NODES {
            return;
        }
        match child {
            usvg::Node::Group(sub) => {
                let abs = xf(sub.abs_transform());
                let id = cx.doc.alloc_id();
                let name = if sub.id().is_empty() { "Group".to_string() } else { sub.id().to_string() };
                let mut n = Node::new(id, NodeKind::Group, name, 0.0, 0.0);
                n.opacity = sub.opacity().get();
                if cx.doc.insert(n, parent, None).is_err() {
                    continue;
                }
                cx.count += 1;
                add_group(cx, sub, id, abs, depth + 1);
                if cx.doc.node(id).is_some_and(|n| n.children.is_empty()) {
                    let _ = cx.doc.remove(id);
                } else {
                    let _ = cx.doc.fit_to_children(id);
                }
            }
            usvg::Node::Path(p) => {
                if !p.is_visible() {
                    continue;
                }
                let abs = xf(p.abs_transform());
                let mut path = to_bez(p.data());
                path.apply_affine(parent_abs.inverse() * abs);
                // Place the vector at its bbox origin in the parent.
                let bb = path.bounding_box();
                if !(bb.x0.is_finite() && bb.y0.is_finite()) {
                    continue;
                }
                path.apply_affine(Affine::translate((-bb.x0, -bb.y0)));
                let mut net = VectorNetwork::from_path(&path);
                let even = p.fill().is_some_and(|f| f.rule() == usvg::FillRule::EvenOdd);
                for r in &mut net.regions {
                    r.winding = if even { WindingRule::EvenOdd } else { WindingRule::NonZero };
                }
                if p.fill().is_none() {
                    net.regions.clear();
                }
                let id = cx.doc.alloc_id();
                let name = if p.id().is_empty() { "Vector".to_string() } else { p.id().to_string() };
                let mut n = Node::new(id, NodeKind::Vector { network: net }, name, bb.width(), bb.height());
                n.transform = Affine::translate((bb.x0, bb.y0));
                let abs_bbox = figcraft_geom::transform_rect_bbox(parent_abs, bb);
                // Paint coordinates are absolute: express them in the vector's box.
                n.fills = p.fill().and_then(|f| paint(f.paint(), f.opacity().get(), abs_bbox, Affine::IDENTITY)).into_iter().collect();
                if let Some(s) = p.stroke() {
                    n.strokes = paint(s.paint(), s.opacity().get(), abs_bbox, Affine::IDENTITY).into_iter().collect();
                    n.stroke.weight = f64::from(s.width().get());
                    n.stroke.align = StrokeAlign::Center;
                    n.stroke.join = match s.linejoin() {
                        usvg::LineJoin::Round => StrokeJoin::Round,
                        usvg::LineJoin::Bevel => StrokeJoin::Bevel,
                        _ => StrokeJoin::Miter,
                    };
                    let cap = match s.linecap() {
                        usvg::LineCap::Round => StrokeCap::Round,
                        usvg::LineCap::Square => StrokeCap::Square,
                        usvg::LineCap::Butt => StrokeCap::None,
                    };
                    n.stroke.cap_start = cap;
                    n.stroke.cap_end = cap;
                    n.stroke.miter_limit = f64::from(s.miterlimit().get());
                    if let Some(d) = s.dasharray() {
                        n.stroke.dashes = d.iter().map(|v| f64::from(*v)).collect();
                    }
                } else {
                    n.strokes.clear();
                }
                if cx.doc.insert(n, parent, None).is_ok() {
                    cx.count += 1;
                }
            }
            usvg::Node::Text(t) => {
                let flat = t.flattened();
                add_group(cx, flat, parent, parent_abs, depth + 1);
            }
            usvg::Node::Image(img) => {
                let bytes = match img.kind() {
                    usvg::ImageKind::PNG(b) | usvg::ImageKind::JPEG(b) | usvg::ImageKind::GIF(b) | usvg::ImageKind::WEBP(b) => b.as_ref().clone(),
                    usvg::ImageKind::SVG(_) => continue,
                };
                let hash = simple_hash(&bytes);
                let abs = xf(img.abs_transform());
                let local = parent_abs.inverse() * abs;
                let size = img.size();
                let id = cx.doc.alloc_id();
                let mut n = Node::new(id, NodeKind::Rectangle, "Image", f64::from(size.width()), f64::from(size.height()));
                n.transform = local;
                n.fills = vec![Paint::image(hash.clone(), figcraft_color::ImageScaleMode::Fill)];
                let (w, h) = (size.width() as u32, size.height() as u32);
                cx.doc.images.insert(hash, figcraft_doc::ImageData { bytes: std::sync::Arc::new(bytes), width: w, height: h });
                if cx.doc.insert(n, parent, None).is_ok() {
                    cx.count += 1;
                }
            }
        }
    }
}

fn simple_hash(b: &[u8]) -> String {
    // FNV-1a 128 (two lanes); the format crate re-keys by sha256 on save if needed.
    let mut h1: u64 = 0xcbf2_9ce4_8422_2325;
    let mut h2: u64 = 0x8422_2325_cbf2_9ce4;
    for (i, x) in b.iter().enumerate() {
        h1 ^= u64::from(*x);
        h1 = h1.wrapping_mul(0x0100_0000_01b3);
        h2 ^= u64::from(*x).wrapping_add(i as u64);
        h2 = h2.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h1:016x}{h2:016x}")
}

/// Import SVG bytes as a new frame under `parent` at `(x, y)`. Returns the frame id.
pub fn import_svg(doc: &mut Document, data: &[u8], parent: NodeId, x: f64, y: f64, name: &str) -> Result<NodeId, ImportError> {
    let opts = usvg::Options::default();
    let tree = usvg::Tree::from_data(data, &opts).map_err(|e| ImportError(e.to_string()))?;
    let size = tree.size();
    let frame = doc
        .create(parent, NodeKind::Frame, name, x, y, f64::from(size.width()), f64::from(size.height()))
        .map_err(|e| ImportError(e.to_string()))?;
    if let Some(f) = doc.node_mut(frame) {
        f.fills.clear();
    }
    let mut cx = Ctx { doc, count: 0 };
    add_group(&mut cx, tree.root(), frame, Affine::IDENTITY, 0);
    Ok(frame)
}
