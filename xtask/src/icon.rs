//! The app icon, generated from code with FigCraft's own renderer (original artwork,
//! MIT OR Apache-2.0): a rounded tile in the app colour with a paper "frame" mark — three stacked
//! layer cards and a cursor. Writes assets/app-icon/*.png, the hicolor theme and figcraft.ico.

use figcraft_color::{Color, Paint};
use figcraft_doc::{Document, NodeKind};

pub const APP_COLOR: &str = "0EA5C6";
const INK: &str = "0B0B0C";
const PAPER: &str = "EFE9DC";

fn build() -> Option<Document> {
    let mut d = Document::new("icon");
    let p = d.first_page()?;
    let bg = d.create(p, NodeKind::Rectangle, "tile", 0.0, 0.0, 512.0, 512.0).ok()?;
    let n = d.get_mut(bg).ok()?;
    n.corners.radii = [112.0; 4];
    n.corners.smoothing = 0.6;
    n.fills = vec![Paint::solid(Color::from_hex(APP_COLOR)?)];
    // Three stacked cards (layers), back to front.
    for (i, (x, y, w, h)) in [(150.0, 96.0, 250.0, 200.0), (122.0, 150.0, 250.0, 200.0), (94.0, 204.0, 250.0, 200.0)].iter().enumerate() {
        let c = d.create(p, NodeKind::Rectangle, "card", *x, *y, *w, *h).ok()?;
        let n = d.get_mut(c).ok()?;
        n.corners.radii = [28.0; 4];
        n.corners.smoothing = 0.6;
        n.fills = vec![Paint::solid(Color::from_hex(PAPER)?)];
        n.strokes = vec![Paint::solid(Color::from_hex(INK)?)];
        n.stroke.weight = 14.0;
        n.stroke.align = figcraft_doc::StrokeAlign::Inside;
        if i == 2 {
            // Front card: a frame grid "#".
            for (gx, gy, gw, gh) in
                [(150.0, 228.0, 14.0, 152.0), (250.0, 228.0, 14.0, 152.0), (118.0, 260.0, 178.0, 14.0), (118.0, 330.0, 178.0, 14.0)]
            {
                let g = d.create(p, NodeKind::Rectangle, "grid", gx, gy, gw, gh).ok()?;
                let gn = d.get_mut(g).ok()?;
                gn.fills = vec![Paint::solid(Color::from_hex(INK)?)];
                gn.corners.radii = [7.0; 4];
            }
        }
    }
    // Cursor.
    let cur = figcraft_doc::VectorNetwork::from_points(
        &[
            (0.0, 0.0).into(),
            (0.0, 120.0).into(),
            (32.0, 92.0).into(),
            (52.0, 136.0).into(),
            (72.0, 128.0).into(),
            (52.0, 84.0).into(),
            (92.0, 82.0).into(),
        ],
        true,
    );
    let c = d.create(p, NodeKind::Vector { network: cur }, "cursor", 318.0, 300.0, 92.0, 136.0).ok()?;
    let n = d.get_mut(c).ok()?;
    n.fills = vec![Paint::solid(Color::from_hex(INK)?)];
    n.strokes = vec![Paint::solid(Color::from_hex(PAPER)?)];
    n.stroke.weight = 10.0;
    n.stroke.align = figcraft_doc::StrokeAlign::Outside;
    n.stroke.join = figcraft_doc::StrokeJoin::Round;
    Some(d)
}

pub fn generate(root: &std::path::Path) -> bool {
    let Some(doc) = build() else { return false };
    let Some(page) = doc.first_page() else { return false };
    let dir = root.join("assets/app-icon");
    let _ = std::fs::create_dir_all(dir.join("hicolor"));
    let render = |size: u32| -> Option<image::RgbaImage> {
        let view = figcraft_render::View { origin: (0.0, 0.0), scale: f64::from(size) / 512.0, width: size, height: size };
        let opts = figcraft_render::Options { background: Some(Color::TRANSPARENT), ..Default::default() };
        let pix = figcraft_render::render_view(&doc, page, &view, &opts)?;
        image::RgbaImage::from_raw(size, size, figcraft_render::to_rgba8(&pix))
    };
    let mut ok = true;
    for size in [16u32, 32, 48, 64, 128, 256, 512, 1024] {
        let Some(img) = render(size) else { return false };
        let name = if size == 1024 { "figcraft-1024.png".to_string() } else { format!("figcraft-{size}.png") };
        ok &= img.save(dir.join(&name)).is_ok();
        if size <= 512 {
            let hd = dir.join(format!("hicolor/{size}x{size}/apps"));
            let _ = std::fs::create_dir_all(&hd);
            ok &= img.save(hd.join("ai.storyteller.figcraft.png")).is_ok();
        }
    }
    // Windows .ico (PNG-compressed entries).
    let mut ico = Vec::new();
    let sizes = [16u32, 32, 48, 256];
    let pngs: Vec<Vec<u8>> = sizes
        .iter()
        .filter_map(|s| {
            let img = render(*s)?;
            let mut out = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
            Some(out)
        })
        .collect();
    ico.extend_from_slice(&[0, 0, 1, 0, pngs.len() as u8, 0]);
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (s, png) in sizes.iter().zip(&pngs) {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        ico.extend_from_slice(&[b, b, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in &pngs {
        ico.extend_from_slice(png);
    }
    ok &= std::fs::write(dir.join("figcraft.ico"), ico).is_ok();
    // Scalable SVG (same artwork, written by FigCraft's SVG exporter).
    let tops: Vec<_> = doc.node(page).map(|p| p.children.clone()).unwrap_or_default();
    match figcraft_svg::export_svg(&doc, &tops, &Default::default()) {
        Some(svg) => {
            let sd = dir.join("hicolor/scalable/apps");
            let _ = std::fs::create_dir_all(&sd);
            ok &= std::fs::write(dir.join("figcraft.svg"), &svg).is_ok();
            ok &= std::fs::write(sd.join("ai.storyteller.figcraft.svg"), &svg).is_ok();
        }
        None => ok = false,
    }
    eprintln!("icon: wrote {}", dir.display());
    ok
}
