use super::*;

const SIGNER: &str = "23D2160CBF2E23068EDE049889E2185954CC08D8";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/system-extension/signed-repository")
            .join(name),
    )
    .unwrap()
}

fn verify_fixture_at_signed_date(
    keyring: &[u8],
    fingerprints: &[&str],
) -> anyhow::Result<SignedRelease> {
    let now = DateTime::parse_from_rfc3339("2026-10-18T00:00:00Z")?.with_timezone(&Utc);
    verify_inrelease_with_keyring_at(&fixture("InRelease"), keyring, fingerprints, now)
}

#[test]
fn synthetic_signed_bookworm_release_is_accepted_and_scoped() {
    let release =
        verify_fixture_at_signed_date(&fixture("archive-keyring.gpg"), &[SIGNER]).unwrap();
    assert_eq!(release.origin, "Debian");
    assert_eq!(release.suite, "bookworm");
    assert_eq!(release.codename, "bookworm");
    assert!(release.architectures.iter().any(|arch| arch == "amd64"));
    assert!(
        release
            .components
            .iter()
            .any(|component| component == "main")
    );
    assert_eq!(
        release.sha256.get("main/binary-amd64/Packages.gz"),
        Some(&(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
            0
        ))
    );
}

#[test]
fn signed_release_rejects_tampering_and_untrusted_signers() {
    let mut changed = fixture("InRelease");
    let original = b"Codename: bookworm";
    let replacement = b"Codename: bullseye";
    let offset = changed
        .windows(original.len())
        .position(|window| window == original)
        .unwrap();
    changed[offset..offset + original.len()].copy_from_slice(replacement);
    assert!(
        verify_inrelease_with_keyring(&changed, &fixture("archive-keyring.gpg"), &[SIGNER])
            .is_err()
    );
    assert!(
        verify_inrelease_with_keyring(
            &fixture("InRelease"),
            BOOKWORM_KEYRING,
            BOOKWORM_FINGERPRINTS
        )
        .is_err()
    );
}

#[test]
fn unpinned_archive_keys_cannot_block_a_valid_pinned_signer() {
    let mut keyring = BOOKWORM_KEYRING.to_vec();
    keyring.extend(fixture("archive-keyring.gpg"));
    let release = verify_fixture_at_signed_date(&keyring, &[SIGNER]);
    assert!(release.is_ok());
}

#[test]
fn signed_index_size_and_digest_are_enforced() {
    let release =
        verify_fixture_at_signed_date(&fixture("archive-keyring.gpg"), &[SIGNER]).unwrap();
    assert!(verify_index_bytes(&release, "main/binary-amd64/Packages.gz", b"").is_ok());
    assert!(verify_index_bytes(&release, "main/binary-arm64/Packages.gz", b"").is_err());
    assert!(verify_index_bytes(&release, "../Packages.gz", b"").is_err());
    assert!(verify_index_bytes(&release, "main/binary-amd64/Packages.gz", b"tampered").is_err());
}

#[test]
fn release_without_valid_until_uses_a_bounded_date_window() {
    let signed_date = Utc::now() - chrono::Duration::days(1);
    let date = signed_date.format("%a, %d %b %Y %H:%M:%S +0000");
    let body = format!(
        "Origin: Debian\nLabel: Debian\nSuite: bookworm\nCodename: bookworm\nDate: {date}\nArchitectures: amd64 arm64\nComponents: main\nSHA256:\n e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 0 main/binary-amd64/Packages.gz\n"
    );
    let release = parse_release_body_at(&body, Utc::now())
        .expect("Valid-Until is optional for Debian stable");
    assert_eq!(
        release.valid_until_unix,
        (signed_date + chrono::Duration::days(180)).timestamp()
    );
}

#[test]
fn release_without_valid_until_is_rejected_after_180_days() {
    let date = (Utc::now() - chrono::Duration::days(181)).format("%a, %d %b %Y %H:%M:%S +0000");
    let body = format!(
        "Origin: Debian\nLabel: Debian\nSuite: bookworm\nCodename: bookworm\nDate: {date}\nArchitectures: amd64 arm64\nComponents: main\nSHA256:\n e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 0 main/binary-amd64/Packages.gz\n"
    );
    assert!(parse_release_body_at(&body, Utc::now()).is_err());
}

#[test]
fn bookworm_release_accepts_debian_lifecycle_suite_label() {
    let date = Utc::now().format("%a, %d %b %Y %H:%M:%S +0000");
    let body = format!(
        "Origin: Debian\nLabel: Debian\nSuite: oldstable\nCodename: bookworm\nDate: {date}\nArchitectures: amd64 arm64\nComponents: main\nSHA256:\n e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 0 main/binary-amd64/Packages.gz\n"
    );
    let release = parse_release_body_at(&body, Utc::now()).unwrap();
    assert_eq!(release.suite, "oldstable");
    assert_eq!(release.codename, "bookworm");
}

#[test]
fn supplied_valid_until_is_enforced_even_when_date_is_recent() {
    let date = Utc::now().format("%a, %d %b %Y %H:%M:%S +0000");
    let expiry = (Utc::now() - chrono::Duration::minutes(1)).format("%a, %d %b %Y %H:%M:%S +0000");
    assert!(
        validate_release_window_at(&date.to_string(), Some(&expiry.to_string()), Utc::now())
            .is_err()
    );
}
