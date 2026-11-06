use super::readiness_state;

fn provider(checked_at_unix_ms: u64) -> crate::persistence::harness_settings::DetectedHarness {
    crate::persistence::harness_settings::DetectedHarness {
        status: "unavailable".into(),
        version: None,
        executable: None,
        diagnostic: None,
        ready: false,
        models: Vec::new(),
        default_model: None,
        model_catalog_fresh: false,
        configuration_required: true,
        implementation_available: false,
        capability_snapshot: None,
        checked_at_unix_ms,
    }
}

#[test]
fn fresh_negative_provider_probe_is_not_ready_but_stale_evidence_is_unknown() {
    let now = 1_000_000;
    assert_eq!(
        readiness_state("codex", None, &provider(now), now),
        crate::core::product_capabilities::ReadinessState::NotReady
    );
    assert_eq!(
        readiness_state(
            "codex",
            None,
            &provider(
                now.saturating_sub(
                    crate::persistence::harness_settings::DISCOVERY_FRESHNESS_MS + 1
                )
            ),
            now
        ),
        crate::core::product_capabilities::ReadinessState::Unavailable
    );
}
