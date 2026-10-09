use super::*;

#[test]
fn hex_roundtrip_and_shorthands() {
    assert_eq!(Color::from_hex("#0D99FF").unwrap().to_hex(), "0D99FF");
    assert_eq!(Color::from_hex("f").unwrap().to_hex(), "FFFFFF");
    assert_eq!(Color::from_hex("ab").unwrap().to_hex(), "ABABAB");
    assert_eq!(Color::from_hex("abc").unwrap().to_hex(), "AABBCC");
    assert_eq!(Color::from_hex("FF000080").unwrap().to_rgba8(), [255, 0, 0, 128]);
    assert!(Color::from_hex("xyz").is_none());
    assert!(Color::from_hex("12345").is_none());
}

#[test]
fn css_parsing() {
    assert_eq!(Color::parse_css("rgb(255, 0, 0)").unwrap().to_hex(), "FF0000");
    assert_eq!(Color::parse_css("rgba(0 0 255 / 50%)").unwrap().to_rgba8(), [0, 0, 255, 128]);
    assert_eq!(Color::parse_css("hsl(120, 100%, 50%)").unwrap().to_hex(), "00FF00");
    assert_eq!(Color::parse_css("white").unwrap(), Color::WHITE);
}

#[test]
fn hsb_roundtrip() {
    for hex in ["0D99FF", "F24822", "14AE5C", "000000", "FFFFFF", "808080", "9747FF"] {
        let c = Color::from_hex(hex).unwrap();
        assert_eq!(Hsb::from_color(c).to_color().to_hex(), hex);
        assert_eq!(Hsl::from_color(c).to_color().to_hex(), hex);
    }
}

#[test]
fn gradient_sampling() {
    let g = Gradient {
        kind: GradientKind::Linear,
        stops: vec![ColorStop { position: 1.0, color: Color::WHITE }, ColorStop { position: 0.0, color: Color::BLACK }],
        from: (0.0, 0.0),
        to: (1.0, 0.0),
        width: (0.0, 1.0),
    };
    assert_eq!(g.sample(0.0), Color::BLACK);
    assert_eq!(g.sample(1.0), Color::WHITE);
    assert_eq!(g.sample(0.5).to_hex(), "808080");
}

#[test]
fn paint_serde_shape() {
    let p = Paint::solid(Color::from_hex("FF0000").unwrap());
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["type"], "solid");
    let back: Paint = serde_json::from_value(j).unwrap();
    assert_eq!(back, p);
    let g: Paint = serde_json::from_str(r#"{"type":"gradient","kind":"radial","stops":[],"from":[0.5,0.5],"to":[1,0.5],"width":[0.5,1]}"#).unwrap();
    assert!(matches!(g.kind, PaintKind::Gradient(_)));
}

#[test]
fn blend_names() {
    assert_eq!(BlendMode::from_name("multiply"), Some(BlendMode::Multiply));
    assert_eq!(BlendMode::from_name("Color burn"), Some(BlendMode::ColorBurn));
    assert_eq!(BlendMode::from_name("PASS_THROUGH"), Some(BlendMode::PassThrough));
}

#[test]
fn nan_inputs_are_sanitized() {
    let c = Color::new(f32::NAN, 2.0, -1.0, f32::INFINITY);
    assert_eq!(c.to_rgba8(), [0, 255, 0, 0]);
}

#[test]
fn colour_deserializes_from_many_forms() {
    let a: Color = serde_json::from_str("\"#FF0000\"").unwrap();
    let b: Color = serde_json::from_str("{\"r\":1,\"g\":0,\"b\":0}").unwrap();
    let c: Color = serde_json::from_str("[1,0,0,1]").unwrap();
    assert_eq!(a, b);
    assert_eq!(b, c);
    assert!(serde_json::from_str::<Color>("\"nope\"").is_err());
}
