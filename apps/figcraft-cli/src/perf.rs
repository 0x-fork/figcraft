//! `figcraft-cli perf`: a synthetic large document and timings of the hot paths.

use std::process::ExitCode;
use std::time::Instant;

use figcraft_doc::color::{Color, Paint};
use figcraft_doc::geom::Point;
use figcraft_doc::{Document, NodeId, NodeKind};
use serde_json::json;

use crate::{CliError, CliResult};

const FRAME_W: f64 = 400.0;
const FRAME_H: f64 = 300.0;
const GAP: f64 = 100.0;

/// `shapes` layers spread over `frames` frames in a grid; every tenth layer is text.
pub fn build(shapes: usize, frames: usize) -> CliResult<(Document, NodeId)> {
    let mut doc = Document::new("Perf");
    let page = doc.first_page().ok_or_else(|| CliError::Failed("new document has no page".into()))?;
    let cols = (frames as f64).sqrt().ceil().max(1.0) as usize * 2;
    let per = shapes.div_ceil(frames).max(1);
    let grid = (per as f64).sqrt().ceil().max(1.0) as usize;
    let (cw, ch) = (FRAME_W / grid as f64, FRAME_H / grid as f64);
    let mut made = 0usize;
    let err = |e: figcraft_doc::DocError| CliError::Failed(e.to_string());
    for f in 0..frames {
        let (fx, fy) = ((f % cols) as f64 * (FRAME_W + GAP), (f / cols) as f64 * (FRAME_H + GAP));
        let frame = doc.create(page, NodeKind::Frame, &format!("Frame {}", f + 1), fx, fy, FRAME_W, FRAME_H).map_err(err)?;
        if let Some(n) = doc.node_mut(frame) {
            n.fills = vec![Paint::solid(Color::WHITE)];
        }
        for k in 0..per {
            if made >= shapes {
                break;
            }
            let (x, y) = ((k % grid) as f64 * cw + 2.0, (k / grid) as f64 * ch + 2.0);
            let hue = (made % 360) as f32 / 360.0;
            let (kind, name) = match made % 10 {
                0 => {
                    let (mut kind, _) = figcraft_engine::cmd::create::kind_for("text").ok_or_else(|| CliError::Failed("no text kind".into()))?;
                    if let NodeKind::Text(t) = &mut kind {
                        t.characters = format!("Item {made}");
                    }
                    (kind, "Text")
                }
                1 | 4 | 7 => (figcraft_engine::cmd::create::kind_for("ellipse").map(|k| k.0).unwrap_or(NodeKind::Rectangle), "Ellipse"),
                _ => (NodeKind::Rectangle, "Rectangle"),
            };
            let id = doc.create(frame, kind, name, x, y, (cw - 4.0).max(1.0), (ch - 4.0).max(1.0)).map_err(err)?;
            if let Some(n) = doc.node_mut(id)
                && name != "Text"
            {
                n.fills = vec![Paint::solid(Color::new(hue, 0.5, 1.0 - hue, 1.0))];
                n.corners.radii = [4.0; 4];
            }
            made += 1;
        }
    }
    Ok((doc, page))
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

pub fn run(shapes: usize, frames: usize) -> CliResult<ExitCode> {
    let mut rows: Vec<(String, f64, String)> = Vec::new();

    let t = Instant::now();
    let (doc, page) = build(shapes, frames)?;
    rows.push(("build document (doc API)".into(), ms(t), format!("{} nodes", doc.node_count())));

    let bounds = doc.page_content_bounds(page).ok_or_else(|| CliError::Failed("empty page".into()))?;
    let t = Instant::now();
    let full = crate::files::render_page(&doc, page, 1.0)?;
    rows.push(("render full page @1x".into(), ms(t), format!("{}×{} px", full.width(), full.height())));
    drop(full);

    let view = figcraft_render::View { origin: (bounds.x0 + 200.0, bounds.y0 + 150.0), scale: 2.0, width: 1920, height: 1080 };
    let t = Instant::now();
    let mut n = 0;
    for _ in 0..5 {
        if figcraft_render::render_view(&doc, page, &view, &Default::default()).is_some() {
            n += 1;
        }
    }
    rows.push(("render 1920×1080 view @2x (avg of 5)".into(), ms(t) / f64::from(n.max(1)), "zoomed into the first frames".into()));

    let fit = figcraft_render::View {
        origin: (bounds.x0, bounds.y0),
        scale: (1920.0 / bounds.width()).min(1080.0 / bounds.height()),
        width: 1920,
        height: 1080,
    };
    let t = Instant::now();
    let _ = figcraft_render::render_view(&doc, page, &fit, &Default::default());
    rows.push(("render 1920×1080 zoom-to-fit".into(), ms(t), format!("scale {:.3}", fit.scale)));

    let hits = 10_000;
    let mut found = 0;
    let t = Instant::now();
    for i in 0..hits {
        let fx = (i * 7919 % 1000) as f64 / 1000.0;
        let fy = (i * 104_729 % 1000) as f64 / 1000.0;
        let p = Point::new(bounds.x0 + bounds.width() * fx, bounds.y0 + bounds.height() * fy);
        if doc.hit_test(page, p, 1.0).is_some() {
            found += 1;
        }
    }
    let total = ms(t);
    rows.push((format!("hit test ×{hits}"), total, format!("{:.1} µs each, {found} hits", total * 1000.0 / hits as f64)));

    let t = Instant::now();
    let bytes = figcraft_format::save_bytes(&doc, None).map_err(|e| CliError::Failed(e.to_string()))?;
    rows.push(("save .figcraft".into(), ms(t), format!("{:.1} MB", bytes.len() as f64 / 1e6)));
    let t = Instant::now();
    let (back, _) = figcraft_format::load_bytes(&bytes).map_err(|e| CliError::Failed(e.to_string()))?;
    rows.push(("load .figcraft".into(), ms(t), format!("{} nodes", back.node_count())));
    drop(back);

    let kids = doc.node(page).map(|p| p.children.clone()).unwrap_or_default();
    let t = Instant::now();
    let svg = figcraft_svg::export_svg(&doc, &kids, &Default::default()).unwrap_or_default();
    rows.push(("export page as SVG".into(), ms(t), format!("{:.1} MB", svg.len() as f64 / 1e6)));
    drop(svg);

    // Command throughput through the engine (undo snapshots included).
    let mut s = figcraft_engine::Session::with_document(doc);
    let first = kids.first().copied();
    let t = Instant::now();
    let cmds = 200;
    let mut ok = 0;
    for i in 0..cmds {
        ok +=
            usize::from(s.execute("node.create", &json!({"type": "rectangle", "parent": first, "x": i, "y": i, "width": 10, "height": 10})).is_ok());
    }
    let total = ms(t);
    rows.push((format!("node.create ×{cmds} via Session::execute"), total, format!("{:.2} ms each, {ok} ok", total / f64::from(cmds))));
    let _ = s.execute("select.none", &json!({}));
    let t = Instant::now();
    let _ = s.execute("select.all", &json!({}));
    let _ = s.execute("transform.move", &json!({"dx": 10, "dy": 0}));
    rows.push(("select all + move top-level frames".into(), ms(t), format!("{} selected", s.selection().len())));
    let t = Instant::now();
    let _ = s.undo();
    rows.push(("undo".into(), ms(t), String::new()));

    println!(
        "figcraft perf — {shapes} shapes in {frames} frames ({} {})",
        std::env::consts::OS,
        if cfg!(debug_assertions) { "debug build" } else { "release build" }
    );
    let w = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(10);
    println!("{:<w$}  {:>10}  notes", "operation", "ms");
    println!("{}", "-".repeat(w + 30));
    for (name, t, note) in rows {
        println!("{name:<w$}  {t:>10.1}  {note}");
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    #[test]
    fn builds_the_requested_size() {
        let (doc, page) = super::build(1000, 10).unwrap();
        assert_eq!(doc.node(page).unwrap().children.len(), 10);
        // page + 10 frames + 1000 shapes
        assert_eq!(doc.node_count(), 1 + 10 + 1000);
    }
}
