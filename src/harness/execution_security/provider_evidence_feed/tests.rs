//! Feeds tests for the interim provider evidence feed (issue #591 piece 3).

use super::*;

const GOOD_HEX: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn target_word() -> Option<&'static str> {
    target_platform_word()
}

/// Builds a lock with the given `provider_cli` profile JSON fragment.
fn lock_with(fragment: serde_json::Value) -> Vec<u8> {
    serde_json::json!({
        "schema_version": 1,
        "image_set_version": "test",
        "profiles": {
            "general": [],
            "provider_cli": fragment
        }
    })
    .to_string()
    .into_bytes()
}

fn entry(platform: &str, reference: &str, digest: &str) -> serde_json::Value {
    serde_json::json!({"platform": platform, "reference": reference, "digest": digest})
}

#[test]
fn released_lock_shaping_without_the_key_yields_no_pin() {
    // Universal current state: no provider_cli key at all.
    let lock = serde_json::json!({"schema_version":1,"profiles":{"general":[]}}).to_string();
    assert_eq!(extract_provider_image_pin(lock.as_bytes()), None);
}

#[test]
fn garbage_shapes_fail_closed_to_absence() {
    for fragment in [
        serde_json::json!("just a scalar"),               // not an array
        serde_json::json!([{"platform": null}]),          // wrong types
        serde_json::json!([42]),                          // junk entry
        serde_json::json!([{"platform": "linux/amd64"}]), // missing pin fields
    ] {
        let bytes = lock_with(fragment.clone());
        assert_eq!(
            extract_provider_image_pin(&bytes),
            None,
            "fragment {fragment}"
        );
    }
    assert_eq!(
        extract_provider_image_pin(b"{not even json"),
        None,
        "non-json bytes"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
mod supported_host {
    use super::*;

    #[test]
    fn matching_platform_entry_is_extracted_through_the_shared_grammar() {
        let word = target_word().unwrap();
        let lock = lock_with(serde_json::json!([entry(
            word,
            &format!("ghcr.io/acme/provider-cli@sha256:{GOOD_HEX}"),
            &format!("sha256:{GOOD_HEX}")
        )]));
        let pin = extract_provider_image_pin(&lock).expect("valid published pin must extract");
        assert!(pin.reference.ends_with(&format!("@sha256:{GOOD_HEX}")));
        assert_eq!(pin.digest, format!("sha256:{GOOD_HEX}"));
    }

    #[test]
    fn wrong_platform_entries_do_not_cross_bind() {
        // A pin published for the OTHER linux arch must not admit this one:
        // arch binding is enforced at extraction, not just documented.
        let mine = target_word().unwrap();
        let other = if mine == "linux/amd64" {
            "linux/arm64"
        } else {
            "linux/amd64"
        };
        let lock = lock_with(serde_json::json!([entry(
            other,
            &format!("ghcr.io/acme/provider-cli@sha256:{GOOD_HEX}"),
            &format!("sha256:{GOOD_HEX}")
        )]));
        assert_eq!(extract_provider_image_pin(&lock), None);
    }

    #[test]
    fn malformed_candidates_in_a_good_profile_are_ignored_fail_closed() {
        let other_hex = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let word = target_word().unwrap();
        let mixed_embedded = format!("ghcr.io/acme/provider-cli@sha256:{other_hex}");
        let local_short = format!("sha256:{}abcd", "1".repeat(60));
        let upper = format!(
            "sha256:{}",
            "0000111122223333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF"
        );
        let cases: [(String, String); 4] = [
            // Embedded digest conflicts with the digest field.
            (mixed_embedded, format!("sha256:{GOOD_HEX}")),
            // Digest is a tag, not a digest.
            (
                "ghcr.io/acme/provider-cli".to_string(),
                "latest".to_string(),
            ),
            // Registry-less localhost spelling + 31-ish short hash.
            ("localhost:5000/provider".to_string(), local_short),
            // Uppercase hex is not the canonical lowercase grammar.
            ("ghcr.io/acme/provider-cli".to_string(), upper),
        ];
        for (reference, digest) in cases {
            let lock = lock_with(serde_json::json!([entry(word, &reference, &digest)]));
            assert_eq!(
                extract_provider_image_pin(&lock),
                None,
                "candidate ({reference}, {digest}) must hold admission"
            );
        }
    }

    #[test]
    fn tagged_reference_with_clean_digest_admits_per_the_shared_grammar() {
        // Contract check on the SINGLE shared grammar (session layer): a
        // reference carries presentation only - identification is by digest,
        // so `repo:v1` plus a clean sha256 digest IS a valid pin. The feed
        // inherits the grammar instead of inventing a rival one.
        let word = target_word().unwrap();
        let lock = lock_with(serde_json::json!([entry(
            word,
            "ghcr.io/acme/provider-cli:v1",
            &format!("sha256:{GOOD_HEX}")
        )]));
        assert!(extract_provider_image_pin(&lock).is_some());
    }

    #[test]
    fn golden_paper_path_selection_then_launch_from_one_gathering() {
        use crate::harness::execution_security::CliProvider;
        use crate::harness::execution_security::profiles::task_execution_block_reason_ctx;
        use crate::harness::execution_security::provider_contained_launch::plan_contained_launch;
        let word = target_word().unwrap();
        let ref_pinned = format!("ghcr.io/acme/provider-cli@sha256:{GOOD_HEX}");
        let lock = lock_with(serde_json::json!([entry(
            word,
            &ref_pinned,
            &format!("sha256:{GOOD_HEX}")
        )]));
        let evidence = gather_provider_evidence(ProviderEvidenceInputs {
            lock_bytes: &lock,
            engine_verified: true,
            hook_containment_attested: true,
            platform_os: PlatformFact::Linux,
        });
        for id in ["codex", "claude"] {
            assert_eq!(
                task_execution_block_reason_ctx(id, &evidence),
                None,
                "selection must admit with complete gathered evidence ({id})"
            );
        }
        let plan = plan_contained_launch(
            CliProvider::Codex,
            &evidence,
            "paper-1",
            vec![String::from("codex")],
        )
        .expect("launch planning must succeed from the same gathered evidence");
        assert!(plan.spec.is_sealed());
        assert_eq!(plan.spec.image_reference, ref_pinned);
        assert_eq!(plan.spec.image_digest, format!("sha256:{GOOD_HEX}"));
    }
}

#[test]
fn facet_passthrough_and_platform_mapping_are_pure() {
    // Facets pass through verbatim regardless of lock content; the platform
    // fact travels as the caller detected it (the detector itself is host-
    // dependent and asserted separately).
    let blank = b"{}".to_vec();
    let inputs = ProviderEvidenceInputs {
        lock_bytes: &blank,
        engine_verified: true,
        hook_containment_attested: false,
        platform_os: PlatformFact::Macos,
    };
    let ev = gather_provider_evidence(inputs);
    assert!(ev.engine_verified);
    assert!(!ev.hook_containment_attested);
    assert_eq!(ev.platform_os, PlatformFact::Macos);
    assert_eq!(ev.image_pin, None);
}

#[test]
fn platform_detector_agrees_with_the_build() {
    let fact = detect_platform_fact();
    let host_linux = std::env::consts::OS == "linux";
    let arch_ok = target_word().is_some();
    let expected = if host_linux && arch_ok {
        PlatformFact::Linux
    } else {
        PlatformFact::Unknown
    };
    assert_eq!(fact, expected, "detector must bind os AND arch");
}

#[test]
fn first_reader_snapshot_defaults_to_the_historic_obstruction_when_unprovisioned() {
    // User-facing invariant, conditioned honestly: only asserted while this
    // test process carries no interim operator assertions.
    let unprovisioned = std::env::var("KOOLADE_PROVIDER_ENGINE_VERIFIED").is_err()
        && std::env::var("KOOLADE_PROVIDER_HOOK_CONTAINMENT_ATTESTED").is_err();
    if !unprovisioned {
        eprintln!("SKIP: process carries interim provider assertions");
        return;
    }
    // Under this guard the default-off engine/attestation switches are
    // the decisive protectors for Claude (each false forces Obstructed
    // regardless of host or pin publication). Codex uses the host route.
    for id in ["codex", "claude"] {
        assert_eq!(
            super::super::profiles::profile_block_reason(
                id,
                crate::harness::ExecutionProfile::Worker
            ),
            super::super::profiles::task_execution_block_reason(id),
            "{id}: unprovisioned deployment must reproduce the legacy notice"
        );
    }
}
