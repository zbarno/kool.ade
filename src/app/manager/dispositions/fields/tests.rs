#[test]
fn arbitration_text_round_trips_multiline_unicode_and_legacy_raw_values() {
    let exact = "first line\nsecond line — evidence quote";
    let encoded = super::encode(exact).unwrap();
    assert!(encoded.starts_with("json:"));
    assert!(!encoded.contains('\n'));
    assert_eq!(super::decode(&encoded), exact);

    let legacy = "first line\nsecond line — evidence quote";
    assert_eq!(super::decode(legacy), legacy);
    assert_eq!(super::decode("\"hello\""), "\"hello\"");
}
