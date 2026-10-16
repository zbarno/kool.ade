use super::super::super::super::LifecycleStore;
use sha2::Digest as _;

pub(super) fn declined_authorization(
    receipt_uid: &str,
    feature_uid: &str,
    requirement_id: &str,
    event_uid: &str,
    old_target: &str,
    proposed_target: &str,
) -> crate::core::lifecycle::FeatureDeltaAuthorizationV1 {
    crate::core::lifecycle::FeatureDeltaAuthorizationV1 {
        schema_version: 1,
        receipt_uid: receipt_uid.to_owned(),
        feature_uid: feature_uid.to_owned(),
        source_spec_sequence: 1,
        requirement_id: requirement_id.to_owned(),
        event_uid: event_uid.to_owned(),
        old_target_sha256: Some(format!("{:x}", sha2::Sha256::digest(old_target.as_bytes()))),
        proposed_target_sha256: Some(format!(
            "{:x}",
            sha2::Sha256::digest(proposed_target.as_bytes())
        )),
        resulting_product_sha256: None,
        decision: crate::core::lifecycle::SpecDeltaOperatorChoiceV1::Declined,
        actor: crate::core::lifecycle::GateActor::User,
        recorded_at_ms: 1,
    }
}

pub(super) fn assert_reverse_references(
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    requirement_id: &str,
    references: &[String],
) {
    for reference in references {
        let matches = lifecycle
            .requirements_for_specification_reference(feature_uid, reference)
            .unwrap();
        assert_eq!(matches.len(), 1, "reverse lookup failed for {reference}");
        assert_eq!(matches[0].requirement_id, requirement_id);
    }
}
