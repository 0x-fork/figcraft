use figcraft_engine::live::parse_color;

#[test]
fn parse_color_rejects_signs_and_non_hex() {
    assert_eq!(parse_color("#+1+2+3"), None);
    assert_eq!(parse_color("#-1-2-3"), None);
    assert_eq!(parse_color("#0D99FF"), Some((13, 153, 255)));
}
