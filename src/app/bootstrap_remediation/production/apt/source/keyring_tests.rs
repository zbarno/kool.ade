use super::*;

const TEST_KEYRING: &[u8] =
    include_bytes!("../../../../../../containers/keys/debian-bookworm-archive-keyring.gpg");
const TEST_FINGERPRINTS: &[&str] = &[
    "B8B80B5B623EAB6AD8775C45B7C5D7D6350947F8",
    "05AB90340C0C5E797F44A8C8254CF3B5AEC0A8F0",
    "04B54C3CDCA79751B16BC6B5225629DF75B188BD",
    "5E04A1E3223A19A20706E20F9904613D4CCE68C6",
    "4D64FEC119C2029067D6E791F8D2585B8783D481",
];

#[test]
fn keyring_requires_the_exact_approved_primary_key_set() {
    verify_keyring_fingerprints(TEST_KEYRING, TEST_FINGERPRINTS).unwrap();
    assert!(verify_keyring_fingerprints(TEST_KEYRING, &["00"; 1]).is_err());
    assert!(verify_keyring_fingerprints(TEST_KEYRING, &[]).is_err());
}

#[test]
fn parses_the_ascii_armored_key_format_used_by_apt_sources() {
    let (keys, _) = PublicOrSecret::from_reader_many(Cursor::new(TEST_KEYRING)).unwrap();
    let first = keys.into_iter().next().unwrap().unwrap();
    let PublicOrSecret::Public(key) = first else {
        panic!("test archive key must be public")
    };
    let fingerprint = format!("{:X}", key.fingerprint());
    let armored = key
        .to_armored_bytes(Default::default())
        .expect("test public key should armor");
    verify_keyring_fingerprints(&armored, &[&fingerprint]).unwrap();
}
