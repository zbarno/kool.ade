use super::{validate_record_collection, validate_record_path};

#[test]
fn lifecycle_record_collections_are_separate_and_revision_fenced() {
    for collection in [
        "lifecycle_features",
        "lifecycle_tasks",
        "lifecycle_events",
        "lifecycle_plans",
        "lifecycle_plan_reviews",
        "lifecycle_plan_authors",
        "lifecycle_plan_confirmations",
        "lifecycle_pr_work_items",
        "opportunities",
        "opportunity_decisions",
        "self_improvements",
        "runtime-receipts",
    ] {
        let path = format!("state/{collection}/00000000-0000-0000-0000-000000000001.json");
        assert!(validate_record_collection(&format!("state/{collection}")).is_ok());
        assert!(validate_record_path(&path).is_ok());
    }
    assert!(validate_record_collection("state/lifecycle").is_err());
    assert!(
        validate_record_path("state/lifecycle/features/00000000-0000-0000-0000-000000000001.json")
            .is_err()
    );
}

#[test]
fn opportunity_and_self_improvement_record_paths_are_validated_by_shape() {
    assert!(validate_record_path("state/opportunity_discovery_settings.json").is_ok());
    assert!(validate_record_path("state/other_settings.json").is_err());
    assert!(validate_record_path("state/opportunity_quota/2026-10-13.json").is_ok());
    assert!(validate_record_path("state/opportunity_quota/2026-13-40.json").is_err());
    assert!(validate_record_path(
        "state/opportunity_dedup/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.json"
    )
    .is_ok());
    assert!(
        validate_record_path("state/opportunity_dedup/00000000-0000-0000-0000-000000000001.json")
            .is_err()
    );
    assert!(validate_record_path(
        "state/self_improvement_events/00000000-0000-0000-0000-000000000001/00000000-0000-0000-0000-000000000002.json"
    )
    .is_ok());
    assert!(
        validate_record_collection(
            "state/self_improvement_events/00000000-0000-0000-0000-000000000001"
        )
        .is_ok()
    );
}
