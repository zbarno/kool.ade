use super::{
    CapabilityState, CapabilityStatus, ProductCapabilityCatalogV1, ReadinessEvidence,
    ReadinessState,
};

#[test]
fn compiled_catalog_has_unique_ids_and_a_real_build_identity_or_unknown() {
    let catalog = ProductCapabilityCatalogV1::compiled().unwrap();
    let ids = catalog
        .capabilities
        .iter()
        .map(|item| item.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), catalog.capabilities.len());
    let answer = catalog.lookup("runtime test docker");
    assert_eq!(answer.matches[0].capability.id, "repository-runtime-qa");
    assert_eq!(
        answer.matches[0].state,
        if answer.build_commit.is_some() {
            CapabilityState::ImplementedInBuild
        } else {
            CapabilityState::Unknown
        }
    );
    assert!(
        answer
            .build_commit
            .as_deref()
            .is_none_or(|commit| matches!(commit.len(), 40 | 64))
    );
}

#[test]
fn readiness_requires_unique_evidence_for_the_exact_build_and_probe() {
    const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let _identity = crate::build_identity::override_for_test("0.1.0", COMMIT);
    let catalog = ProductCapabilityCatalogV1::compiled().unwrap();
    let evidence = ReadinessEvidence {
        capability_id: "self-improvement-lifecycle".into(),
        readiness_probe_id: "self-improvement-lifecycle-v1".into(),
        build_commit: COMMIT.into(),
        probe_evidence_sha256: "c".repeat(64),
        state: ReadinessState::Ready,
    };
    assert_eq!(
        catalog
            .lookup_with_readiness(
                "self improvement lifecycle",
                std::slice::from_ref(&evidence)
            )
            .matches[0]
            .state,
        CapabilityState::VerifiedReady
    );
    let mut stale = evidence.clone();
    stale.build_commit = "b".repeat(40);
    assert_eq!(
        catalog
            .lookup_with_readiness("self improvement lifecycle", &[stale])
            .matches[0]
            .state,
        CapabilityState::ImplementedInBuild
    );
    let mut invalid_build_ref = evidence.clone();
    invalid_build_ref.build_commit = "not-a-commit".into();
    assert_eq!(
        catalog
            .lookup_with_readiness("self improvement lifecycle", &[invalid_build_ref])
            .matches[0]
            .state,
        CapabilityState::ImplementedInBuild
    );
    let mut unavailable = evidence.clone();
    unavailable.state = ReadinessState::Unavailable;
    assert_eq!(
        catalog
            .lookup_with_readiness("self improvement lifecycle", &[unavailable])
            .matches[0]
            .state,
        CapabilityState::Unknown
    );
    let mut conflicting = evidence.clone();
    conflicting.state = ReadinessState::NotReady;
    assert_eq!(
        catalog
            .lookup_with_readiness("self improvement lifecycle", &[evidence, conflicting])
            .matches[0]
            .state,
        CapabilityState::Unknown
    );
}

#[test]
fn unmatched_lookup_is_unknown_and_planned_is_distinct() {
    let catalog = ProductCapabilityCatalogV1::compiled().unwrap();
    assert_eq!(
        catalog.lookup("made up imaginary tool xyz").confidence,
        "unknown"
    );
    assert_eq!(
        catalog.lookup("repository architecture").matches[0].state,
        CapabilityState::Planned
    );
}

#[test]
fn lookup_output_is_bounded() {
    let mut catalog = ProductCapabilityCatalogV1::compiled().unwrap();
    for index in 0..32 {
        let mut item = catalog.capabilities[0].clone();
        item.id = format!("extra-{index}");
        item.terms.push("common query".into());
        catalog.capabilities.push(item);
    }
    let answer = catalog.lookup("common query");
    assert!(answer.matches.len() <= 5);
}

#[test]
fn all_code_and_test_refs_exist_and_registered_probe_ids_stay_stable() {
    let catalog = ProductCapabilityCatalogV1::compiled().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for capability in &catalog.capabilities {
        for path in capability
            .implementation_refs
            .iter()
            .chain(&capability.test_refs)
        {
            assert!(root.join(path).is_file(), "stale capability ref: {path}");
        }
    }
    let active: serde_json::Value = serde_json::from_str(include_str!(
        "../../../assets/capabilities/active-capabilities.json"
    ))
    .unwrap();
    let entries = active["capabilities"].as_array().unwrap();
    for entry in entries {
        let id = entry["capabilityId"].as_str().unwrap();
        let probe = entry["readinessProbeId"].as_str().unwrap();
        assert!(catalog.capabilities.iter().any(|capability| {
            capability.id == id && capability.readiness_probe_id.as_deref() == Some(probe)
        }));
    }
    assert!(catalog.capabilities.iter().any(|capability| {
        capability.id == "self-improvement-lifecycle"
            && capability.status == CapabilityStatus::Implemented
    }));
    assert_eq!(
        catalog
            .capabilities
            .iter()
            .find(|capability| capability.id == "provider-routing-and-role-isolation")
            .and_then(|capability| capability.readiness_probe_id.as_deref()),
        Some("manager-provider-route-v1")
    );
    for capability in &catalog.capabilities {
        if let Some(probe) = capability.readiness_probe_id.as_deref() {
            assert!(super::registered_probes::contains(&capability.id, probe));
            if capability.id == "self-improvement-lifecycle" {
                assert!(entries.iter().any(|entry| {
                    entry["capabilityId"] == capability.id && entry["readinessProbeId"] == probe
                }));
            }
        }
    }
}

#[test]
fn readiness_evidence_rejects_unregistered_provider_probe_pairs() {
    const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert!(
        ReadinessEvidence::from_registered_probe_state(
            "provider-routing-and-role-isolation".into(),
            "invented-provider-probe".into(),
            COMMIT.into(),
            "b".repeat(64),
            ReadinessState::NotReady,
        )
        .is_err()
    );
}

#[test]
fn provider_route_evidence_distinguishes_unavailable_and_unverified_states() {
    const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let _identity = crate::build_identity::override_for_test("0.1.0", COMMIT);
    let catalog = ProductCapabilityCatalogV1::compiled().unwrap();
    let unavailable = ReadinessEvidence::from_registered_probe_state(
        "provider-routing-and-role-isolation".into(),
        "manager-provider-route-v1".into(),
        COMMIT.into(),
        "d".repeat(64),
        ReadinessState::NotReady,
    )
    .unwrap();
    assert_eq!(
        catalog
            .lookup_with_readiness("provider routing and role isolation", &[unavailable])
            .matches[0]
            .state,
        CapabilityState::ImplementedButNotReady
    );
    assert_eq!(
        catalog
            .lookup("provider routing and role isolation")
            .matches[0]
            .state,
        CapabilityState::ImplementedInBuild
    );
}

#[test]
fn every_supported_product_route_is_in_the_stable_catalog_baseline() {
    const EXPECTED_IDS: &[&str] = &[
        "self-improvement-lifecycle",
        "project-specification-planning",
        "task-board-and-conversations",
        "manager-opportunity-discovery",
        "repository-runtime-qa",
        "workspace-and-repository-management",
        "questions-and-decisions",
        "provider-routing-and-role-isolation",
        "planner-reviewer-role-workflows",
        "git-branch-and-pull-request-workflows",
        "docker-execution",
        "code-review-and-repair",
        "local-operational-storage",
        "product-documentation-opportunity-scan",
        "revision-aware-repository-knowledge",
        "shared-multiuser-task-service",
    ];
    let catalog = ProductCapabilityCatalogV1::compiled().unwrap();
    let actual = catalog
        .capabilities
        .iter()
        .map(|item| item.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, EXPECTED_IDS.iter().copied().collect());
    assert_eq!(
        catalog.lookup("shared multiuser task service").matches[0].state,
        CapabilityState::Unsupported
    );
}

#[test]
fn public_snapshot_pins_catalog_to_the_actual_build_or_marks_identity_unknown() {
    let snapshot_json = ProductCapabilityCatalogV1::public_snapshot();
    assert!(snapshot_json.len() < 16 * 1024);
    let snapshot: serde_json::Value = serde_json::from_str(&snapshot_json).unwrap();
    assert_eq!(snapshot["catalogStatus"], "validated");
    assert_eq!(snapshot["catalogSha256"].as_str().unwrap().len(), 64);
    if let Some(commit) = snapshot["buildCommit"].as_str() {
        assert!(matches!(commit.len(), 40 | 64));
        assert!(snapshot["buildVersion"].as_str().is_some());
    } else {
        assert!(snapshot["buildVersion"].is_null());
    }
    assert!(
        snapshot["catalog"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability["id"] == "self-improvement-lifecycle")
    );
}
