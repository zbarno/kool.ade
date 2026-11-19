use super::hex_encode;

fn pin_with_real_key(minimum_generation: u64) -> String {
    use ed25519_dalek::SigningKey;
    let key = SigningKey::from_bytes(&[71_u8; 32]);
    let public_hex: String = key
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "{{\"minimumGeneration\": {minimum_generation}, \"keys\": [\n\
         {{\"keyId\": \"11111111-2222-4333-8444-555555555555\", \"publicKeyHex\": \"{public_hex}\"}}]}}"
    )
}

#[test]
fn hex_round_trip_matches_format_encoding() {
    let bytes: [u8; 8] = [0x00, 0x01, 0xab, 0xcd, 0xef, 0x0f, 0xfe, 0x12];
    assert_eq!(hex_encode(&bytes), "0001abcdef0ffe12");
}

#[test]
fn seed_and_key_parsing_reject_bad_shapes() {
    assert!(super::parse_signing_seed("ed25519-seed-hex:zz").is_err());
    assert!(super::parse_signing_seed("short").is_err());
    let key = ed25519_dalek::SigningKey::from_bytes(&[71_u8; 32]);
    let good: String = key
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert!(super::decode_public_key_hex(&good).is_ok());
    assert!(super::decode_public_key_hex(&"abc".repeat(20)).is_err());
    assert!(super::decode_public_key_hex(&"AB".repeat(32)).is_err());
    assert!(super::decode_public_key_hex(&"zz".repeat(32)).is_err());
    assert!(super::parse_positive_u64("0", "GENERATION").is_err());
    assert!(super::parse_positive_u64("3", "GENERATION").unwrap() == 3);
    assert!(super::parse_positive_u64("x", "GENERATION").is_err());
}

#[test]
fn trust_pin_parses_and_rejects_drift() {
    let parsed: super::TrustPinDocument =
        serde_json::from_str(&pin_with_real_key(4)).expect("valid pin");
    assert_eq!(parsed.minimum_generation, 4);
    assert_eq!(parsed.pinned_keys().expect("keys").len(), 1);

    assert!(
        serde_json::from_str::<super::TrustPinDocument>(
            "{\"minimumGeneration\": 1, \"keys\": [], \"surprise\": 1}"
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<super::TrustPinDocument>(
            "{\"minimumGeneration\": 1, \"keyRing\": []}"
        )
        .is_err()
    );
}

#[test]
fn argument_collection_counts_positionals_exactly() {
    assert!(super::collect_args::<2>("one two".split_whitespace().map(str::to_owned)).is_ok());
    assert!(super::collect_args::<2>("one".split_whitespace().map(str::to_owned)).is_err());
    assert!(super::collect_args::<1>("a b".split_whitespace().map(str::to_owned)).is_err());
}
