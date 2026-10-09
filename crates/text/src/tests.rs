use super::*;
use figcraft_doc::{TextAlignH, TextData, TextDecoration};

fn have_fonts() -> bool {
    db().write().unwrap().default_face().is_some() || {
        scan_system_fonts();
        db().write().unwrap().default_face().is_some()
    }
}

#[test]
fn lays_out_single_line() {
    if !have_fonts() {
        eprintln!("no fonts; skipping");
        return;
    }
    let mut t = TextData::new("Hello");
    t.style.font_size = 20.0;
    let l = layout(&t, None, None);
    assert_eq!(l.lines.len(), 1);
    assert!(l.glyphs.len() >= 5);
    assert!(l.width > 30.0 && l.width < 100.0, "{}", l.width);
    assert!(l.height > 20.0 && l.height < 30.0, "{}", l.height);
    // Caret moves right.
    assert!(l.caret(3).0 > l.caret(1).0);
    assert_eq!(l.hit(-5.0, 5.0), 0);
    assert_eq!(l.hit(1000.0, 5.0), 5);
}

#[test]
fn newline_and_wrap() {
    if !have_fonts() {
        return;
    }
    let mut t = TextData::new("one two three four five six\nseven");
    t.auto_resize = figcraft_doc::TextAutoResize::Height;
    let l = layout(&t, Some(60.0), None);
    assert!(l.lines.len() >= 3, "{}", l.lines.len());
    assert!(l.lines.iter().all(|ln| ln.width <= 60.0 + 1e-6 || ln.glyph_end - ln.glyph_start <= 1));
    // Auto width ignores the box.
    t.auto_resize = figcraft_doc::TextAutoResize::WidthAndHeight;
    assert_eq!(layout(&t, Some(60.0), None).lines.len(), 2);
}

#[test]
fn alignment_and_decoration() {
    if !have_fonts() {
        return;
    }
    let mut t = TextData::new("Hi");
    t.auto_resize = figcraft_doc::TextAutoResize::None;
    t.align_h = TextAlignH::Right;
    t.style.decoration = TextDecoration::Underline;
    let l = layout(&t, Some(200.0), None);
    let last = l.glyphs.last().unwrap();
    assert!((last.x + last.advance - 200.0).abs() < 0.5);
    assert_eq!(l.decorations.len(), 1);
}

#[test]
fn empty_and_hostile_inputs() {
    if !have_fonts() {
        return;
    }
    let mut t = TextData::new("");
    let l = layout(&t, None, None);
    assert_eq!(l.lines.len(), 1);
    assert!(l.height > 0.0);
    t.characters = "\n\n".into();
    assert_eq!(layout(&t, None, None).lines.len(), 3);
    t.style.font_size = f64::NAN;
    t.characters = "x".into();
    let _ = layout(&t, Some(f64::NAN), Some(-1.0));
    t.runs.push(figcraft_doc::StyleRun { start: 5, end: 2, style: Default::default() });
    let _ = layout(&t, None, None);
    let mut emoji = TextData::new("a😀b 漢字 ß");
    emoji.style.case = figcraft_doc::TextCase::Upper;
    let l = layout(&emoji, None, None);
    assert!(!l.glyphs.is_empty());
}

#[test]
fn truncate() {
    if !have_fonts() {
        return;
    }
    let mut t = TextData::new("a very long line of text that will not fit at all");
    t.auto_resize = figcraft_doc::TextAutoResize::Truncate;
    let l = layout(&t, Some(80.0), Some(16.0));
    assert!(l.truncated);
    assert_eq!(l.lines.len(), 1);
}

#[test]
fn glyph_paths_have_area() {
    if !have_fonts() {
        return;
    }
    use figcraft_geom::Shape;
    let t = TextData::new("O");
    let l = layout(&t, None, None);
    let p = l.to_path();
    assert!(p.area().abs() > 1.0);
}
