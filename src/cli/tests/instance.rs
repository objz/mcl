use super::parse_resolution;

#[test]
fn parses_valid_resolution() {
    assert_eq!(
        parse_resolution("1920x1080").expect("should parse"),
        (1920, 1080)
    );
}

#[test]
fn rejects_invalid_resolution_format() {
    assert!(parse_resolution("1920").is_err());
    assert!(parse_resolution("1920xa").is_err());
    assert!(parse_resolution("0x1080").is_err());
}
