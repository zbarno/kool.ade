use crate::core::planning_work::PlanConfirmationV1;

fn payload() -> PlanConfirmationV1 {
    PlanConfirmationV1 {
        schema_version: 1,
        feature_uid: "feature-uid".into(),
        snapshot_content_hash: "current-hash".into(),
        snapshot_spec_revision: "current-revision".into(),
        pin_is_published: true,
        ready: true,
        cause: None,
        blockers: vec![],
        evaluation_prompted_for: None,
        announced_ready: Some(true),
        evaluated_at_unix_ms: 0,
    }
}

/// Production boundary: the retry control must bind to the exact approved
/// feature snapshot shown on this card. This direct predicate test proves all
/// identity fields are required; app-level review recovery tests cover command
/// authorization. No external dependencies or setup cost.
#[test]
fn retry_action_matches_only_the_same_feature_snapshot_and_revision() {
    let payload = payload();
    let matches = |id: &str, uid: &str, hash: &str, revision: &str| {
        super::snapshot_binding_matches(id, uid, hash, revision, "FEAT-7", &payload)
    };

    assert!(matches(
        "FEAT-7",
        "feature-uid",
        "current-hash",
        "current-revision"
    ));
    assert!(!matches(
        "FEAT-8",
        "feature-uid",
        "current-hash",
        "current-revision"
    ));
    assert!(!matches(
        "FEAT-7",
        "another-feature",
        "current-hash",
        "current-revision"
    ));
    assert!(!matches(
        "FEAT-7",
        "feature-uid",
        "older-hash",
        "current-revision"
    ));
    assert!(!matches(
        "FEAT-7",
        "feature-uid",
        "current-hash",
        "older-revision"
    ));
}
