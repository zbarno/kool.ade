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

#[test]
fn verify_pinned_refuses_absence_garbage_and_external_signatures() {
    use ed25519_dalek::{Signer, SigningKey};

    let lowercase_hex = |bytes: &[u8]| -> String {
        let mut rendered = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            rendered.push_str(&format!("{byte:02x}"));
        }
        rendered
    };

    use koolade::computer_use::runtime_manifest::{
        BUNDLED_RUNTIME_MANIFEST_FILENAME, RuntimeManifestV1,
    };
    use sha2::{Digest, Sha256};

    // Absence of the fixed-name manifest is a refusal, not a pass-through.
    let empty = tempfile::TempDir::new().unwrap();
    let absent_error = super::verify_pinned_bundle(empty.path()).err().unwrap();
    assert!(
        absent_error
            .to_string()
            .contains(BUNDLED_RUNTIME_MANIFEST_FILENAME),
        "{absent_error:?}"
    );

    // Garbage in the fixed slot is a refusal.
    let garbage = tempfile::TempDir::new().unwrap();
    std::fs::write(
        garbage.path().join(BUNDLED_RUNTIME_MANIFEST_FILENAME),
        "not json",
    )
    .unwrap();
    let garbage_error = super::verify_pinned_bundle(garbage.path()).err().unwrap();
    assert!(
        garbage_error.to_string().contains("DecodeFailed"),
        "{garbage_error:?}"
    );

    // Impersonates the application-pinned signer KEY ID with an externally
    // generated key: the envelope must trip signature verification, proving
    // the pinned public key (held as a repository secret, not in this code)
    // is what admits manifests. This is the strongest positive-control lane
    // CI can run; happy-path admission happens only inside the promotion
    // pipeline that holds the secret.
    let resources = tempfile::TempDir::new().unwrap();
    let qemu = b"foreign-ci-fake-qemu-bytes";
    std::fs::create_dir_all(resources.path().join("qemu")).unwrap();
    std::fs::write(resources.path().join("qemu/qemu-system-x86_64"), qemu).unwrap();
    let mut digest_acc = Sha256::new();
    digest_acc.update(qemu);
    let digest_hex = lowercase_hex(digest_acc.finalize().as_slice());
    let now_ms = chrono::Utc::now().timestamp().max(0) as u64 * 1000 + 1;
    let mut manifest: RuntimeManifestV1 = serde_json::from_value(serde_json::json!({
        "schemaVersion": 1,
        "releaseVersion": "test-packaging-0.1.0",
        "generation": 1,
        "issuedAtUnixMs": now_ms,
        "expiresAtUnixMs": now_ms + 30 * 86_400_000,
        "signerKeyId": "9b1c6e04-7a2f-4f3d-bc5b-2d4e6f8a0c1e",
        "signatureHex": "00",
        "artifacts": [{
            "artifactId": "0f8a1c2e-3d4b-4a59-8c6e-7d2f9a0b1c3d",
            "kind": "executable",
            "platform": "mac_os_intel",
            "guestArchitecture": "x86_64",
            "bundleRelativePath": "qemu/qemu-system-x86_64",
            "sha256": format!("sha256:{digest_hex}"),
            "sizeBytes": qemu.len() as u64,
            "permissionBits": 0o755,
        }]
    }))
    .unwrap();
    let seed: [u8; 32] = [0xA5; 32];
    let external_key = SigningKey::from_bytes(&seed);
    let signature = external_key.sign(&manifest.signing_payload_bytes().unwrap());
    manifest.signature_hex = lowercase_hex(&signature.to_bytes());
    let mut rendered = serde_json::to_vec_pretty(&manifest).unwrap();
    rendered.push(b'\n');
    std::fs::write(
        resources.path().join(BUNDLED_RUNTIME_MANIFEST_FILENAME),
        rendered,
    )
    .unwrap();

    let external_error = super::verify_pinned_bundle(resources.path()).err().unwrap();
    assert!(
        external_error.to_string().contains("BadSignature"),
        "external key must be refused as misattributed, got {external_error:?}"
    );
}
