use super::approved_comparison_pins;

#[test]
fn comparison_authority_contains_only_bounded_validated_same_repository_git_intents() {
    let mut metadata_items = Vec::new();
    for index in 0..10 {
        let identity = crate::domain::ArtifactIdentity {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            display_id: format!("T{index}"),
            title: format!("Synthetic task {index}"),
            parent_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        };
        let metadata =
            crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new()).unwrap();
        let intent = crate::core::git_intent::GitIntentV1::approve(
            "root".into(),
            format!("approved-{index}"),
            format!("{index:040x}"),
            "main".into(),
            crate::core::git_intent::PublicationModeV1::HoldForReview,
            None,
        )
        .unwrap();
        metadata_items.push(metadata.with_git_intent(Some(&intent)).unwrap());
    }
    let identity = crate::domain::ArtifactIdentity {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_id: "T-other".into(),
        title: "Other repository".into(),
        parent_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    };
    let other_metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new()).unwrap();
    let other_intent = crate::core::git_intent::GitIntentV1::approve(
        "api".into(),
        "api-approved".into(),
        "a".repeat(40),
        "main".into(),
        crate::core::git_intent::PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap();
    metadata_items.push(other_metadata.with_git_intent(Some(&other_intent)).unwrap());

    let pins = approved_comparison_pins(&metadata_items, "root");
    assert_eq!(pins.len(), 8);
    assert!(
        pins.iter()
            .all(|pin| pin.intent.repository_id == "root" && pin.intent.validate().is_ok())
    );
    assert!(
        pins.iter()
            .all(|pin| pin.intent.source_ref.starts_with("approved-"))
    );
}
