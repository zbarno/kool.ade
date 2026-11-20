//! Matrix tests for capability-evidence evaluation (issue #591 slice A).
//!
//! Policy under test: EVERY precondition is conjunctive and fail-closed.
//! The default/evidence-free state must stay obstructed (historic UX), and
//! each single missing evidence must, by itself, hold the block.

use super::*;

fn valid_pin() -> ContainedImagePin {
    ContainedImagePin {
        reference: "ghcr.io/zbarno/koolade-provider-cli".to_string(),
        digest: format!("sha256:{}", "ab".repeat(32)),
    }
}

fn full_evidence() -> ProviderContainmentEvidence {
    ProviderContainmentEvidence {
        platform_os: PlatformFact::Linux,
        engine_verified: true,
        image_pin: Some(valid_pin()),
        hook_containment_attested: true,
    }
}

#[test]
fn fully_evidenced_managed_hook_providers_are_admitted() {
    for provider in [CliProvider::Codex, CliProvider::Claude] {
        assert_eq!(
            evaluate_task_execution(provider, &full_evidence()),
            TaskExecutionVerdict::Runnable,
            "{provider:?}"
        );
    }
}

#[test]
fn evidence_free_state_is_fully_obstructed_fail_closed() {
    // Default evidence = every precondition unevidenced. This is the state
    // of every current deployment and must reproduce the historic block.
    for provider in [CliProvider::Codex, CliProvider::Claude] {
        let verdict = evaluate_task_execution(provider, &ProviderContainmentEvidence::default());
        match verdict {
            TaskExecutionVerdict::Obstructed(ref all) => {
                // Collection order is the evaluation order (stable).
                assert_eq!(
                    all.iter()
                        .map(std::mem::discriminant)
                        .collect::<Vec<_>>()
                        .as_slice(),
                    [
                        std::mem::discriminant(&ContainmentObstruction::UnsupportedPlatform {
                            os: "x",
                            arch: "y"
                        },),
                        std::mem::discriminant(&ContainmentObstruction::EngineNotVerified),
                        std::mem::discriminant(&ContainmentObstruction::ImageNotPinned),
                        std::mem::discriminant(&ContainmentObstruction::HookContainmentNotAttested),
                    ],
                );
            }
            other => panic!("expected obstructed, got {other:?}"),
        }
    }
}

#[test]
fn two_consecutive_holes_report_in_documented_order() {
    let mut e = full_evidence();
    e.engine_verified = false; // hole 2
    e.hook_containment_attested = false; // hole 4
    let verdict = evaluate_task_execution(CliProvider::Codex, &e);
    match verdict {
        TaskExecutionVerdict::Obstructed(all) => {
            assert_eq!(all.len(), 2);
            assert!(matches!(all[0], ContainmentObstruction::EngineNotVerified));
            assert!(matches!(
                all[1],
                ContainmentObstruction::HookContainmentNotAttested
            ));
        }
        other => panic!("expected obstructed, got {other:?}"),
    }
}

#[test]
fn overlong_digest_is_refused_as_a_pin() {
    let e = ProviderContainmentEvidence {
        image_pin: Some(ContainedImagePin {
            reference: valid_pin().reference.clone(),
            digest: format!("sha256:{:065x}", 7usize),
        }),
        ..full_evidence()
    };
    match evaluate_task_execution(CliProvider::Codex, &e) {
        TaskExecutionVerdict::Obstructed(all) => assert!(
            all.iter()
                .any(|o| matches!(o, ContainmentObstruction::InvalidImagePin(_)))
        ),
        other => panic!("expected obstructed, got {other:?}"),
    }
}

#[test]
fn uppercase_hex_digest_is_refused_as_a_pin() {
    let mut hex = vec![b'0'; 64];
    hex[10] = b'A'; // one illegal byte among legal lowercase hex
    let digest = format!("sha256:{}", String::from_utf8(hex).unwrap());
    let e = ProviderContainmentEvidence {
        image_pin: Some(ContainedImagePin {
            reference: valid_pin().reference.clone(),
            digest,
        }),
        ..full_evidence()
    };
    match evaluate_task_execution(CliProvider::Codex, &e) {
        TaskExecutionVerdict::Obstructed(all) => assert!(
            all.iter()
                .any(|o| matches!(o, ContainmentObstruction::InvalidImagePin(_)))
        ),
        other => panic!("expected obstructed, got {other:?}"),
    }
}
#[test]
fn each_single_missing_evidence_alone_holds_the_block() {
    enum Hole {
        Engine,
        Image,
        Attestation,
    }
    let cases = [Hole::Engine, Hole::Image, Hole::Attestation];
    for hole in cases {
        let name = match hole {
            Hole::Engine => "engine",
            Hole::Image => "image",
            _ => "attestation",
        };
        for provider in [CliProvider::Codex, CliProvider::Claude] {
            let mut ev = full_evidence();
            match hole {
                Hole::Engine => ev.engine_verified = false,
                Hole::Image => ev.image_pin = None,
                Hole::Attestation => ev.hook_containment_attested = false,
            }
            let verdict = evaluate_task_execution(provider, &ev);
            match verdict {
                TaskExecutionVerdict::Obstructed(mut all) => {
                    assert_eq!(all.len(), 1, "{name}: exactly one obstruction");
                    let only = all.pop().unwrap();
                    let expected = match name {
                        "engine" => ContainmentObstruction::EngineNotVerified,
                        "image" => ContainmentObstruction::ImageNotPinned,
                        _ => ContainmentObstruction::HookContainmentNotAttested,
                    };
                    assert_eq!(
                        std::mem::discriminant(&only),
                        std::mem::discriminant(&expected),
                        "{name}"
                    );
                }
                other => panic!("{name}: expected obstructed, got {other:?}"),
            }
        }
    }
}

#[test]
fn non_linux_platforms_are_never_admitted_even_with_perfect_other_evidence() {
    for os in [
        PlatformFact::Macos,
        PlatformFact::Windows,
        PlatformFact::Unknown,
    ] {
        let mut ev = full_evidence();
        ev.platform_os = os;
        let verdict = evaluate_task_execution(CliProvider::Codex, &ev);
        match verdict {
            TaskExecutionVerdict::Obstructed(all) => {
                assert!(
                    matches!(
                        all.first(),
                        Some(ContainmentObstruction::UnsupportedPlatform { .. })
                    ),
                    "{os:?} must fail on the platform fact first"
                );
                assert_eq!(all.len(), 1, "platform alone suffices to block");
            }
            other => panic!("{os:?}: expected obstructed, got {other:?}"),
        }
    }
}

#[test]
fn malformed_pins_are_obstructed_not_silently_accepted() {
    let bad_pins = [
        // Tag-only presentation smuggled into the digest slot.
        ContainedImagePin {
            reference: valid_pin().reference,
            digest: "latest".to_string(),
        },
        // Uppercase hex is not the canonical lowercase form.
        ContainedImagePin {
            reference: valid_pin().reference,
            digest: format!("sha256:{}", "AB".repeat(32)),
        },
        // Wrong-length digest.
        ContainedImagePin {
            reference: valid_pin().reference,
            digest: format!("sha256:{}", "ab".repeat(31)),
        },
        // Local registry: an attacker-planted local image is not a pin.
        ContainedImagePin {
            reference: "localhost:5000/koolade-provider-cli".to_string(),
            digest: valid_pin().digest,
        },
    ];
    for pin in &bad_pins {
        let mut ev = full_evidence();
        ev.image_pin = Some(pin.clone());
        let verdict = evaluate_task_execution(CliProvider::Claude, &ev);
        match verdict {
            TaskExecutionVerdict::Obstructed(all) => {
                assert!(
                    matches!(all.as_slice(), [ContainmentObstruction::InvalidImagePin(_)]),
                    "pin {pin:?} produced {all:?}"
                );
            }
            other => panic!("pin {:?} slipped through: {other:?}", pin),
        }
    }
}

#[test]
fn providers_without_managed_hook_exposure_are_out_of_scope() {
    for provider in [
        CliProvider::Pi,
        CliProvider::OpenCode,
        CliProvider::Copilot,
        CliProvider::Antigravity,
    ] {
        // Out of scope under ANY evidence state — both empty and full.
        assert_eq!(
            evaluate_task_execution(provider, &ProviderContainmentEvidence::default()),
            TaskExecutionVerdict::OutOfScope
        );
        assert_eq!(
            evaluate_task_execution(provider, &full_evidence()),
            TaskExecutionVerdict::OutOfScope
        );
    }
}

#[test]
fn obstruction_descriptions_are_nonempty_operational_text() {
    for os in ["macos", "windows", "unidentified"] {
        let o = ContainmentObstruction::UnsupportedPlatform { os, arch: "x86_64" };
        assert!(!o.describe().trim().is_empty());
    }
    let samples = vec![
        ContainmentObstruction::EngineNotVerified,
        ContainmentObstruction::ImageNotPinned,
        ContainmentObstruction::InvalidImagePin("ref:sha256:x".into()),
        ContainmentObstruction::HookContainmentNotAttested,
    ];
    for o in &samples {
        let text = o.describe();
        assert!(text.chars().count() > 15, "{o:?}");
        assert!(!text.starts_with(' ') && !text.ends_with(' '), "{text:?}");
    }
}
