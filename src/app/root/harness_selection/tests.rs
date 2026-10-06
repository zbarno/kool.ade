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
