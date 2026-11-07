use ed25519_dalek::SigningKey;

use super::parse_signing_seed;

#[test]
fn signing_seed_parser_accepts_only_a_fixed_width_ed25519_seed() {
    let encoded = "07".repeat(32);
    assert_eq!(
        parse_signing_seed(&encoded).unwrap(),
        SigningKey::from_bytes(&[7; 32])
    );
    assert!(parse_signing_seed(&format!("{encoded}00")).is_err());
    assert!(parse_signing_seed(&"zz".repeat(32)).is_err());
}

#[test]
fn signing_seed_parser_accepts_an_explicit_format_prefix() {
    let encoded = format!("ed25519-seed-hex:{}", "2a".repeat(32));
    assert_eq!(
        parse_signing_seed(&encoded).unwrap(),
        SigningKey::from_bytes(&[42; 32])
    );
}
