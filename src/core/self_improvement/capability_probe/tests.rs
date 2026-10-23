use super::probe_capability_manifest;

#[path = "registry.rs"]
mod registry;

const CAPABILITY: &str = "self-improvement-lifecycle";

fn manifest(capability_id: &str) -> String {
    serde_json::json!({
        "schemaVersion": 1,
        "capabilities": [{
            "capabilityId": capability_id,
            "readinessProbeId": "self-improvement-lifecycle-v1"
        }]
    })
    .to_string()
}

#[test]
fn active_capability_probe_is_bound_to_the_plan_capability_id_and_inventory() {
    let source = manifest(CAPABILITY);
    let evidence = probe_capability_manifest(CAPABILITY, &source).unwrap();
    assert_eq!(evidence.capability_id, "self-improvement-lifecycle");
    assert_eq!(evidence.readiness_probe_id, "self-improvement-lifecycle-v1");
    assert_eq!(evidence.manifest_sha256.len(), 64);
    assert!(probe_capability_manifest("another-feature-capability", &source).is_err());
}

#[test]
fn active_capability_probe_rejects_invalid_duplicate_and_unregistered_entries() {
    assert!(
        probe_capability_manifest(CAPABILITY, "{\"schemaVersion\":1,\"capabilities\":[]}").is_err()
    );
    assert!(
        probe_capability_manifest("Invalid-Capability", &manifest("Invalid-Capability")).is_err()
    );
    assert!(
        probe_capability_manifest(
            CAPABILITY,
            &format!("{}{}", manifest(CAPABILITY), " ".repeat(64 * 1024))
        )
        .is_err()
    );

    let duplicate = serde_json::json!({
        "schemaVersion": 1,
        "capabilities": [
            {"capabilityId": "first", "readinessProbeId": "probe-a"},
            {"capabilityId": "second", "readinessProbeId": "probe-b"}
        ]
    });
    assert!(probe_capability_manifest(CAPABILITY, &duplicate.to_string()).is_err());

    let duplicate_capability = serde_json::json!({
        "schemaVersion": 1,
        "capabilities": [
            {"capabilityId": CAPABILITY, "readinessProbeId": "self-improvement-lifecycle-v1"},
            {"capabilityId": CAPABILITY, "readinessProbeId": "self-improvement-lifecycle-v1"}
        ]
    });
    assert!(probe_capability_manifest(CAPABILITY, &duplicate_capability.to_string()).is_err());

    let mismatched_probe = serde_json::json!({
        "schemaVersion": 1,
        "capabilities": [{
            "capabilityId": "unrelated-capability",
            "readinessProbeId": "self-improvement-lifecycle-v1"
        }]
    });
    assert!(probe_capability_manifest(CAPABILITY, &mismatched_probe.to_string()).is_err());
}
