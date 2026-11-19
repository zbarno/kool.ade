#![allow(clippy::unwrap_used)]
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use super::{
    application_trust::application_runtime_manifest_trust_v1,
    bundle::{BUNDLED_RUNTIME_MANIFEST_FILENAME, load_bundled_runtime_manifest},
    manifest::{
        MAX_RUNTIME_MANIFEST_BYTES, RUNTIME_MANIFEST_SCHEMA_VERSION, RuntimeArtifactKindV1,
        RuntimeArtifactV1, RuntimeManifestErrorV1, RuntimeManifestPlatformV1,
        RuntimeManifestTrustV1, RuntimeManifestV1,
    },
    verification::verify_runtime_manifest,
};
use crate::computer_use::guest_image::ComputerGuestArchitectureV1;

const TEST_KEY_ID: &str = "11111111-2222-4333-8444-555555555555";
const FOREIGN_KEY_ID: &str = "eeeeeeee-dddd-4fff-8fff-000000000000";
const NOW: u64 = 1_760_000_000_000;

fn signer() -> SigningKey {
    SigningKey::from_bytes(&[71; 32])
}

fn other_signer() -> SigningKey {
    SigningKey::from_bytes(&[72; 32])
}

fn trust_for(key: &VerifyingKey) -> RuntimeManifestTrustV1 {
    RuntimeManifestTrustV1::from_application_pinned_keys([(TEST_KEY_ID.to_owned(), *key)], 1)
        .unwrap()
}

fn trust_with_floor(floor: u64) -> RuntimeManifestTrustV1 {
    RuntimeManifestTrustV1::from_application_pinned_keys(
        [(TEST_KEY_ID.to_owned(), signer().verifying_key())],
        floor,
    )
    .unwrap()
}

fn artifact_id(index: usize) -> String {
    format!("aaaaaaaa-bbbb-4ccc-8ddd-{index:012x}")
}

fn artifact(platform: RuntimeManifestPlatformV1, path: &str) -> RuntimeArtifactV1 {
    let digest_input = format!("artifact-bytes-{path}").into_bytes();
    let size = digest_input.len() as u64;
    let digest = format!("sha256:{:x}", Sha256::digest(&digest_input));
    let id_seed = format!("{platform:?} {path}");
    let id_suffix: String = format!("{:x}", Sha256::digest(id_seed.as_bytes()))
        .chars()
        .take(12)
        .collect();
    RuntimeArtifactV1 {
        artifact_id: format!("aaaaaaaa-bbbb-4ccc-8ddd-{id_suffix}"),
        kind: RuntimeArtifactKindV1::Executable,
        platform,
        guest_architecture: platform.required_guest_architecture(),
        bundle_relative_path: path.to_owned(),
        sha256: digest,
        size_bytes: size,
        permission_bits: 0o755,
    }
}

fn manifest_template() -> RuntimeManifestV1 {
    RuntimeManifestV1 {
        schema_version: RUNTIME_MANIFEST_SCHEMA_VERSION,
        release_version: "2026.11.18-rc1".to_owned(),
        generation: 7,
        issued_at_unix_ms: NOW - 3_600_000,
        expires_at_unix_ms: NOW + 24 * 3_600_000,
        signer_key_id: TEST_KEY_ID.to_owned(),
        artifacts: vec![
            artifact(
                RuntimeManifestPlatformV1::MacOsAppleSilicon,
                RuntimeManifestPlatformV1::MacOsAppleSilicon.canonical_qemu_executable_path(),
            ),
            artifact(
                RuntimeManifestPlatformV1::WindowsX64,
                RuntimeManifestPlatformV1::WindowsX64.canonical_qemu_executable_path(),
            ),
        ],
        signature_hex: "0".repeat(128),
    }
}

/// Build an envelope and sign it with the test key.
fn signed_manifest(mutate: impl FnOnce(&mut RuntimeManifestV1)) -> RuntimeManifestV1 {
    let key = signer();
    let mut manifest = manifest_template();
    mutate(&mut manifest);
    let signature = key.sign(&manifest.signing_payload_bytes().unwrap());
    manifest.signature_hex = hex::encode(signature.to_bytes());
    manifest
}

fn encoded(manifest: &RuntimeManifestV1) -> Vec<u8> {
    serde_json::to_vec(manifest).unwrap()
}

fn expect_error(
    manifest: &RuntimeManifestV1,
    expected: RuntimeManifestErrorV1,
) -> Result<(), RuntimeManifestErrorV1> {
    match verify_runtime_manifest(
        encoded(manifest).as_slice(),
        &trust_for(&signer().verifying_key()),
        NOW,
    ) {
        Err(actual) if std::mem::discriminant(&actual) == std::mem::discriminant(&expected) => {
            Ok(())
        }
        other => panic!("expected {expected:?}, got {other:?}"),
    }
}

fn sign_and_verify(manifest: &RuntimeManifestV1) -> super::VerifiedRuntimeManifestV1 {
    verify_runtime_manifest(
        encoded(manifest).as_slice(),
        &trust_for(&signer().verifying_key()),
        NOW,
    )
    .unwrap()
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let mut out = String::new();
        for byte in bytes.as_ref() {
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[test]
fn verified_manifest_exposes_bound_facts() {
    let verified = sign_and_verify(&signed_manifest(|_| {}));
    assert_eq!(verified.release_version(), "2026.11.18-rc1");
    assert_eq!(verified.signer_key_id(), TEST_KEY_ID);
    assert_eq!(verified.generation(), 7);
    assert_eq!(verified.expires_at_unix_ms(), NOW + 24 * 3_600_000);
    assert!(verified.digest().starts_with("sha256:"));
    assert_eq!(verified.artifacts().count(), 2);

    let artifact = verified
        .qemu_executable_for(RuntimeManifestPlatformV1::MacOsAppleSilicon)
        .expect("apple silicon entry");
    assert_eq!(artifact.bundle_relative_path(), "qemu/qemu-system-aarch64");
    assert_eq!(artifact.digest().len(), 71);
    assert_eq!(artifact.kind(), RuntimeArtifactKindV1::Executable);
    assert_eq!(
        artifact.platform(),
        RuntimeManifestPlatformV1::MacOsAppleSilicon
    );
    assert_eq!(
        artifact.guest_architecture(),
        ComputerGuestArchitectureV1::Aarch64
    );
    assert_eq!(artifact.permission_bits(), 0o755);
}

#[test]
fn digest_and_signing_payload_are_stable_across_serialization() {
    // The parser normalizes member ordering, so re-round-tripping the same
    // envelope — even with members shuffled in the source document — must
    // yield an identical canonical digest and signing payload.
    let base = signed_manifest(|_| {});

    let shuffled_doc = {
        let value = serde_json::to_value(base.clone()).unwrap();
        fn reshuffle(value: serde_json::Value) -> serde_json::Value {
            match value {
                serde_json::Value::Object(map) => {
                    let mut entries: Vec<(String, serde_json::Value)> = map.into_iter().collect();
                    entries.sort_by(|left, right| right.0.cmp(&left.0));
                    serde_json::Value::Object(
                        entries
                            .into_iter()
                            .map(|(key, value)| (key, reshuffle(value)))
                            .collect(),
                    )
                }
                serde_json::Value::Array(items) => {
                    serde_json::Value::Array(items.into_iter().map(reshuffle).collect())
                }
                other => other,
            }
        }
        reshuffle(value)
    };
    let reparsed: RuntimeManifestV1 = serde_json::from_value(shuffled_doc).unwrap();
    assert_eq!(base.digest().unwrap(), reparsed.digest().unwrap());
    assert_eq!(
        base.signing_payload_bytes().unwrap(),
        reparsed.signing_payload_bytes().unwrap()
    );
}

#[test]
fn platform_lookup_resolves_only_canonical_paths() {
    let verified = sign_and_verify(&signed_manifest(|_| {}));
    // Adopted platforms resolve; non-adopted platforms have no entry.
    assert!(
        verified
            .qemu_executable_for(RuntimeManifestPlatformV1::WindowsX64)
            .is_some()
    );
    assert!(
        verified
            .qemu_executable_for(RuntimeManifestPlatformV1::LinuxX86_64)
            .is_none()
    );

    // An entry at a NON-canonical path is invisible to lookup even when the
    // rest of the envelope is perfectly signed.
    let rogue = signed_manifest(|manifest| {
        let mut entry = artifact(
            RuntimeManifestPlatformV1::MacOsIntel,
            "qemu/qemu-system-aarch64",
        );
        entry.artifact_id = FOREIGN_KEY_ID.to_owned();
        manifest.artifacts.push(entry);
    });
    let verified = sign_and_verify(&rogue);
    assert!(
        verified
            .qemu_executable_for(RuntimeManifestPlatformV1::MacOsIntel)
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// Envelope integrity
// ---------------------------------------------------------------------------

#[test]
fn signature_must_come_from_the_pinned_key() {
    let key = other_signer();
    let mut manifest = manifest_template();
    manifest.signature_hex = hex::encode(
        key.sign(&manifest.signing_payload_bytes().unwrap())
            .to_bytes(),
    );
    expect_error(&manifest, RuntimeManifestErrorV1::BadSignature).unwrap();
}

#[test]
fn unknown_signer_key_id_is_rejected_before_verification() {
    let manifest = signed_manifest(|manifest| {
        manifest.signer_key_id = FOREIGN_KEY_ID.to_owned();
    });
    expect_error(&manifest, RuntimeManifestErrorV1::UnknownSigner).unwrap();
}

#[test]
fn tampering_after_signing_breaks_the_signature() {
    type Case = (&'static str, fn(&mut RuntimeManifestV1));
    let cases: &[Case] = &[
        ("artifact digest", |manifest| {
            let entry = manifest.artifacts.last_mut().unwrap();
            // Overwrite eight hex characters past the mandatory prefix.
            let digest = std::mem::take(&mut entry.sha256);
            let (head, rest) = digest.split_at(15);
            let (_, tail) = rest.split_at(8);
            entry.sha256 = format!("{head}deadbeef{tail}");
        }),
        ("generation", |manifest| manifest.generation += 1),
        ("release version", |manifest| {
            // An allowed charset char keeps the case in the signature lane;
            // disallowed glyphs are caught earlier by the header rules.
            manifest.release_version.push('x')
        }),
        ("expiry", |manifest| manifest.expires_at_unix_ms += 1),
        ("artifact identity", |manifest| {
            manifest.artifacts.last_mut().unwrap().artifact_id = artifact_id(99);
        }),
    ];
    for &(label, tamper) in cases {
        let mut manifest = manifest_template();
        manifest.signature_hex = hex::encode(
            signer()
                .sign(&manifest.signing_payload_bytes().unwrap())
                .to_bytes(),
        );
        tamper(&mut manifest);
        expect_error(&manifest, RuntimeManifestErrorV1::BadSignature)
            .unwrap_or_else(|_| panic!("{label}"));
    }
}

#[test]
fn signature_hex_spelling_must_be_lowercase_128_chars() {
    let key = signer();
    let signature = key.sign(&manifest_template().signing_payload_bytes().unwrap());
    let lower = hex::encode(signature.to_bytes());

    let mut upper_case = lower.clone();
    upper_case.make_ascii_uppercase();
    expect_error(
        &signed_manifest_with_signature(upper_case),
        RuntimeManifestErrorV1::InvalidHeader,
    )
    .unwrap();
    expect_error(
        &signed_manifest_with_signature(lower[..lower.len() - 1].to_owned()),
        RuntimeManifestErrorV1::InvalidHeader,
    )
    .unwrap();

    fn signed_manifest_with_signature(hex_sig: String) -> RuntimeManifestV1 {
        let mut manifest = manifest_template();
        manifest.signature_hex = hex_sig;
        manifest
    }
}

#[test]
fn envelope_signed_under_another_domain_does_not_verify() {
    let key = signer();
    let manifest = manifest_template();
    // Mint a cryptographically valid signature over the SAME unsigned fields,
    // but tagged with the guest-image catalog domain instead of the runtime
    // manifest domain.
    let unsigned = serde_json::json!({
        "schemaVersion": manifest.schema_version,
        "releaseVersion": manifest.release_version,
        "generation": manifest.generation,
        "issuedAtUnixMs": manifest.issued_at_unix_ms,
        "expiresAtUnixMs": manifest.expires_at_unix_ms,
        "signerKeyId": manifest.signer_key_id,
        "artifacts": serde_json::to_value(&manifest.artifacts).unwrap(),
    });
    let mut foreign = b"koolade.computer-image-catalog.v1\0".to_vec();
    foreign.extend(serde_json::to_vec(&unsigned).unwrap());
    let spoofed_signature = key.sign(&foreign);
    let mut manifest = manifest_template();
    manifest.signature_hex = hex::encode(spoofed_signature.to_bytes());
    expect_error(&manifest, RuntimeManifestErrorV1::BadSignature).unwrap();
}

#[test]
fn expired_and_below_floor_envelopes_fail_closed() {
    let trust = trust_for(&signer().verifying_key());
    let expired = signed_manifest(|manifest| manifest.expires_at_unix_ms = NOW);
    assert!(matches!(
        verify_runtime_manifest(encoded(&expired).as_slice(), &trust, NOW),
        Err(RuntimeManifestErrorV1::Expired)
    ));

    // Issuance dated ahead of the caller's clock is refused before expiry
    // is even consulted, mirroring the guest-image catalog rule.
    let future = signed_manifest(|manifest| {
        manifest.issued_at_unix_ms = NOW + 60_000;
    });
    assert!(matches!(
        verify_runtime_manifest(encoded(&future).as_slice(), &trust, NOW),
        Err(RuntimeManifestErrorV1::InvalidHeader)
    ));

    let just_alive = signed_manifest(|manifest| {
        manifest.expires_at_unix_ms = NOW + 1;
    });
    assert!(verify_runtime_manifest(encoded(&just_alive).as_slice(), &trust, NOW).is_ok());

    let floored = trust_with_floor(8);
    let current = signed_manifest(|_| {});
    assert!(matches!(
        verify_runtime_manifest(encoded(&current).as_slice(), &floored, NOW),
        Err(RuntimeManifestErrorV1::BelowGenerationFloor)
    ));
}

#[test]
fn envelopes_above_the_byte_cap_fail_before_decoding() {
    let oversized = vec![b'a'; MAX_RUNTIME_MANIFEST_BYTES + 1];
    assert!(matches!(
        verify_runtime_manifest(&oversized, &trust_for(&signer().verifying_key()), NOW),
        Err(RuntimeManifestErrorV1::TooLarge)
    ));
}

// ---------------------------------------------------------------------------
// Header and entry rules
// ---------------------------------------------------------------------------

fn reject(build: impl FnOnce(&mut RuntimeManifestV1), label: &str) {
    expect_error(
        &signed_manifest(build),
        RuntimeManifestErrorV1::InvalidHeader,
    )
    .unwrap_or_else(|_| panic!("{label}"))
}

#[test]
fn header_rules_are_enforced() {
    reject(|manifest| manifest.schema_version = 2, "future schema");
    reject(|manifest| manifest.generation = 0, "zero generation");
    reject(|manifest| manifest.issued_at_unix_ms = 0, "zero issuance");
    reject(
        |manifest| manifest.expires_at_unix_ms = manifest.issued_at_unix_ms,
        "expiry not after issuance",
    );
    reject(
        |manifest| {
            manifest.expires_at_unix_ms = manifest.issued_at_unix_ms + 181 * 24 * 3_600_000;
        },
        "lifetime beyond 180 days",
    );
    reject(
        |manifest| manifest.signer_key_id = "not-a-uuid".to_owned(),
        "invalid signer id",
    );
    reject(
        |manifest| manifest.release_version = "..leading".to_owned(),
        "malformed release version",
    );
    reject(
        |manifest| manifest.release_version = "2026.11.18-rc 1".to_owned(),
        "space in release version",
    );
    reject(
        |manifest| manifest.release_version = "2026-11.18.".to_owned(),
        "trailing dot in release version",
    );
    reject(
        |manifest| manifest.release_version = "\u{3b2}eta".to_owned(),
        "non-ascii release version",
    );
    reject(
        |manifest| manifest.release_version.clear(),
        "empty release version",
    );
    reject(|manifest| manifest.artifacts.clear(), "empty artifact list");

    let mut flood = Vec::new();
    for index in 0..33 {
        let mut entry = artifact(
            RuntimeManifestPlatformV1::WindowsX64,
            &format!("bulk/{index:02}/payload.bin"),
        );
        entry.artifact_id = artifact_id(index + 1);
        entry.kind = RuntimeArtifactKindV1::File;
        entry.permission_bits = 0;
        flood.push(entry);
    }
    reject(
        |manifest| manifest.artifacts = flood.clone(),
        "over-cap artifacts",
    );
}

#[test]
fn duplicate_identities_are_rejected() {
    let twin_id = signed_manifest(|manifest| {
        let mut twin = manifest.artifacts.first().cloned().unwrap();
        twin.bundle_relative_path = "qemu/alt-binary".to_owned();
        manifest.artifacts.push(twin);
    });
    expect_error(&twin_id, RuntimeManifestErrorV1::DuplicateArtifact).unwrap();

    let twin_layout = signed_manifest(|manifest| {
        let mut clone = manifest.artifacts.first().cloned().unwrap();
        clone.artifact_id = artifact_id(2);
        manifest.artifacts.push(clone);
    });
    expect_error(&twin_layout, RuntimeManifestErrorV1::DuplicateArtifact).unwrap();
}

#[test]
fn guest_architecture_must_match_host_platform() {
    let mismatch = signed_manifest(|manifest| {
        manifest.artifacts.first_mut().unwrap().guest_architecture =
            ComputerGuestArchitectureV1::X86_64;
    });
    expect_error(&mismatch, RuntimeManifestErrorV1::InvalidArtifact).unwrap();
}

#[test]
fn executable_permission_bits_follow_the_receipt_contract() {
    for bits in [0o755u32, 0o700, 0o711, 0o744] {
        let granted = signed_manifest(|manifest| {
            for entry in &mut manifest.artifacts {
                entry.permission_bits = bits;
            }
        });
        assert!(
            sign_and_verify(&granted).digest().starts_with("sha256:"),
            "mode {bits:o}"
        );
    }
    for bits in [0o644u32, 0o777, 0o4755, 0o2755, 0o1755] {
        let denied = signed_manifest(|manifest| {
            for entry in &mut manifest.artifacts {
                entry.permission_bits = bits;
            }
        });
        expect_error(&denied, RuntimeManifestErrorV1::InvalidArtifact)
            .unwrap_or_else(|_| panic!("mode {bits:o}"))
    }
}

#[test]
fn non_executable_entries_may_not_carry_modes() {
    let smuggled = signed_manifest(|manifest| {
        let entry = manifest.artifacts.last_mut().unwrap();
        entry.kind = RuntimeArtifactKindV1::DiskImage;
        entry.permission_bits = 0o600;
    });
    expect_error(&smuggled, RuntimeManifestErrorV1::InvalidArtifact).unwrap();

    let passive = signed_manifest(|manifest| {
        let entry = manifest.artifacts.last_mut().unwrap();
        entry.kind = RuntimeArtifactKindV1::File;
        entry.permission_bits = 0;
        entry.bundle_relative_path = "licenses/NOTICE.txt".to_owned();
    });
    assert!(sign_and_verify(&passive).digest().starts_with("sha256:"));
}

#[test]
fn bundle_relative_paths_are_canonical_posix() {
    for path in [
        "/absolute",
        "../escape",
        "a//double",
        "trailing/",
        "with space",
        "unicode-\u{00e9}",
        "back\\slash",
        ".",
        "..",
        "dot/./hidden",
        "nested/a/b/c/d/e/f/g/h/deep",
        &"x".repeat(257),
    ] {
        let poisoned = signed_manifest(|manifest| {
            manifest.artifacts.first_mut().unwrap().bundle_relative_path = path.to_owned();
        });
        expect_error(&poisoned, RuntimeManifestErrorV1::InvalidArtifact)
            .unwrap_or_else(|_| panic!("path {path:?}"));
    }
    let deep_but_legal = signed_manifest(|manifest| {
        manifest.artifacts.first_mut().unwrap().bundle_relative_path =
            "nested/a/b/c/d/e/f/blob".to_owned();
    });
    assert!(
        sign_and_verify(&deep_but_legal)
            .digest()
            .starts_with("sha256:")
    );
}

#[test]
fn size_bounds_apply_to_every_entry() {
    let zero = signed_manifest(|manifest| {
        manifest.artifacts.first_mut().unwrap().size_bytes = 0;
    });
    expect_error(&zero, RuntimeManifestErrorV1::InvalidArtifact).unwrap();

    let gigantic = signed_manifest(|manifest| {
        manifest.artifacts.first_mut().unwrap().size_bytes = 17 * 1024 * 1024 * 1024;
    });
    expect_error(&gigantic, RuntimeManifestErrorV1::InvalidArtifact).unwrap();
}

#[test]
fn digest_spelling_must_be_lowercase_sha256() {
    let candidates = [
        format!("SHA256:{}", "a".repeat(64)),
        format!("sha256:{}", "A".repeat(64)),
        format!("sha256:{}", "a".repeat(63)),
        format!("sha1:{}", "a".repeat(40)),
        "sha256:aabb".to_owned(),
        "sha256:aabbccdd!"
            .repeat(8)
            .chars()
            .take(71)
            .collect::<String>(),
    ];
    for digest in candidates {
        let forged = signed_manifest(|manifest| {
            manifest.artifacts.first_mut().unwrap().sha256 = digest.clone();
        });
        expect_error(&forged, RuntimeManifestErrorV1::InvalidArtifact)
            .unwrap_or_else(|_| panic!("{digest}"));
    }
}

#[test]
fn malformed_envelopes_fail_to_decode_or_parse() {
    let trust = trust_for(&signer().verifying_key());
    assert!(matches!(
        verify_runtime_manifest(b"not json at all", &trust, NOW),
        Err(RuntimeManifestErrorV1::DecodeFailed)
    ));

    let mut value = serde_json::to_value(signed_manifest(|_| {})).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unexpectedField".to_owned(), serde_json::json!(true));
    assert!(matches!(
        verify_runtime_manifest(serde_json::to_vec(&value).unwrap().as_slice(), &trust, NOW),
        Err(RuntimeManifestErrorV1::DecodeFailed)
    ));
}

// ---------------------------------------------------------------------------
// Trust store
// ---------------------------------------------------------------------------

#[test]
fn trust_store_rejects_unsound_pin_lists() {
    assert!(RuntimeManifestTrustV1::from_application_pinned_keys([], 1).is_err());
    assert!(
        RuntimeManifestTrustV1::from_application_pinned_keys(
            [("not-a-uuid".to_owned(), signer().verifying_key())],
            1
        )
        .is_err()
    );
    assert!(
        RuntimeManifestTrustV1::from_application_pinned_keys(
            [(TEST_KEY_ID.to_owned(), signer().verifying_key())],
            0
        )
        .is_err()
    );
    assert!(
        RuntimeManifestTrustV1::from_application_pinned_keys(
            [
                (TEST_KEY_ID.to_owned(), signer().verifying_key()),
                (TEST_KEY_ID.to_owned(), signer().verifying_key()),
            ],
            1
        )
        .is_err()
    );
}

#[test]
fn application_trust_does_not_accept_placeholder_key_material() {
    let trust = application_runtime_manifest_trust_v1().unwrap();
    assert_eq!(trust.minimum_generation(), 1);
    let manifest = signed_manifest(|_| {});
    // The test key shares no material with the release key pinned in the app,
    // and even a same-role signature would land under the placeholder id.
    let outcome = verify_runtime_manifest(encoded(&manifest).as_slice(), &trust, NOW);
    assert!(matches!(
        outcome,
        Err(RuntimeManifestErrorV1::UnknownSigner
            | RuntimeManifestErrorV1::BadSignature
            | RuntimeManifestErrorV1::InvalidHeader)
    ));
}

// ---------------------------------------------------------------------------
// Bundle loader
// ---------------------------------------------------------------------------

#[test]
fn bundle_loader_verifies_fixed_name_files() {
    let trust = trust_for(&signer().verifying_key());
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(BUNDLED_RUNTIME_MANIFEST_FILENAME),
        encoded(&signed_manifest(|_| {})),
    )
    .unwrap();
    assert!(
        load_bundled_runtime_manifest(root.path(), &trust, NOW)
            .unwrap()
            .is_some()
    );
}

#[test]
fn bundle_loader_reports_missing_manifest_as_clean_absence() {
    let trust = trust_for(&signer().verifying_key());
    let root = tempfile::tempdir().unwrap();
    assert!(
        load_bundled_runtime_manifest(root.path(), &trust, NOW)
            .unwrap()
            .is_none()
    );
}

#[test]
fn bundle_loader_refuses_unusual_layouts() {
    let trust = trust_for(&signer().verifying_key());

    let shadow = tempfile::tempdir().unwrap();
    let target = shadow.path().join(BUNDLED_RUNTIME_MANIFEST_FILENAME);
    std::fs::write(&target, b"{}").unwrap();
    let symlink_root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            &target,
            symlink_root.path().join(BUNDLED_RUNTIME_MANIFEST_FILENAME),
        )
        .unwrap();
        assert!(matches!(
            load_bundled_runtime_manifest(symlink_root.path(), &trust, NOW),
            Err(RuntimeManifestErrorV1::InvalidLayout)
        ));
    }

    let dir_root = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir_root.path().join(BUNDLED_RUNTIME_MANIFEST_FILENAME)).unwrap();
    assert!(matches!(
        load_bundled_runtime_manifest(dir_root.path(), &trust, NOW),
        Err(RuntimeManifestErrorV1::InvalidLayout)
    ));

    let empty_root = tempfile::tempdir().unwrap();
    std::fs::write(
        empty_root.path().join(BUNDLED_RUNTIME_MANIFEST_FILENAME),
        b"",
    )
    .unwrap();
    assert!(matches!(
        load_bundled_runtime_manifest(empty_root.path(), &trust, NOW),
        Err(RuntimeManifestErrorV1::TooLarge)
    ));

    let absent_root = tempfile::tempdir()
        .unwrap()
        .path()
        .join("definitely-absent");
    assert!(matches!(
        load_bundled_runtime_manifest(absent_root.as_path(), &trust, NOW),
        Err(RuntimeManifestErrorV1::Io)
    ));
}

#[test]
fn loader_surfaces_envelope_errors_from_a_well_formed_layout() {
    let trust = trust_for(&signer().verifying_key());
    let root = tempfile::tempdir().unwrap();
    // Well laid out, but carrying only the unsigned sentinel signature.
    std::fs::write(
        root.path().join(BUNDLED_RUNTIME_MANIFEST_FILENAME),
        encoded(&manifest_template()),
    )
    .unwrap();
    assert!(matches!(
        load_bundled_runtime_manifest(root.path(), &trust, NOW),
        Err(RuntimeManifestErrorV1::BadSignature)
    ));
}
