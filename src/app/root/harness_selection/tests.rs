use super::*;

#[test]
fn removed_saved_harness_is_reported_instead_of_silently_routing_to_pi() {
    let harness = UnavailableHarness("codex".into());
    assert_eq!(harness.label(), "codex (unavailable)");
    let error = harness.check_available().unwrap_err();
    assert!(
        error
            .detail()
            .contains("saved default coding harness 'codex'")
    );
    assert!(error.detail().contains("choose an available harness"));
}

#[test]
fn saved_supported_harness_ids_resolve_to_their_adapters() {
    assert!(resolve(Some("pi")).label().starts_with("pi "));
    assert!(resolve(Some("codex")).label().starts_with("codex "));
    assert!(resolve(Some("claude")).label().starts_with("claude "));
}
