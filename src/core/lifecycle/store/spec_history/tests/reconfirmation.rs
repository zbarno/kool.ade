use super::*;

#[test]
// Production boundary: explicit feature approval, immutable confirmations and
// lifecycle bridging after a product refresh must agree. Unit record validation
// cannot prove the command preserves the original lifecycle consent. Synthetic
// local Git/PlanningStore fixture only, moderate setup, no external services.
fn reconfirming_the_same_plan_uses_current_product_without_rewriting_lifecycle_consent() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    let (uid, markdown) = traceability::support::reviewed_feature(
        &fixture,
        &mut store,
        "CHG-983",
        "CHG-983-reconfirmation",
    );
    let (feature, snapshot, old, original_events) = {
        let lifecycle = LifecycleStore::new(&store);
        let feature = lifecycle.load_feature(&uid).unwrap();
        let snapshot = crate::core::lifecycle::plan_snapshot::load_persisted(
            &store,
            feature.plan_snapshot_uid.as_deref().unwrap(),
        )
        .unwrap()
        .unwrap();
        let old =
            crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(&store, &snapshot)
                .unwrap()
                .unwrap();
        let original_events = lifecycle.events_for_feature(&uid).unwrap();
        (feature, snapshot, old, original_events)
    };
    let docs = crate::artifacts::product_docs::load_documents(&store)
        .unwrap()
        .unwrap();
    let doc = &docs[0];
    let path = fixture
        .0
        .join("docs/koolade/product")
        .join(&doc.module.path);
    std::fs::write(&path, format!("{}\nUpdated context.\n", doc.content)).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["add", "docs/koolade/product"])
            .current_dir(&fixture.0)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args(["commit", "-qm", "publish refreshed test product context"])
            .current_dir(&fixture.0)
            .status()
            .unwrap()
            .success()
    );
    let reader =
        crate::core::published_specs::PublishedSpecReader::for_primary_repository(&fixture.0)
            .unwrap();
    store.published_spec_pin = Some(reader.pin().unwrap());
    store.remove("state/spec_drafts/current.json").unwrap();
    let proposal = crate::core::apply::private_document_draft_changes(
        &store,
        &[("feature:CHG-983".into(), markdown.clone())],
    )
    .unwrap();
    store.transaction(&proposal, None).unwrap();
    let lifecycle = LifecycleStore::new(&store);
    let workflow = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    assert!(!crate::core::workflow::feature_approved(
        &store, &workflow, "CHG-983"
    ));
    let trigger = lifecycle
        .enqueue_manual_spec_refresh(&uid, "completed-refresh")
        .unwrap();
    let mut draft = Fixture::draft(&store, feature.revision, trigger.uid);
    draft.decision_refs = feature.decision_refs.clone();
    draft.approved_target_hash = feature.approved_plan_sha256.clone();
    let sequence = lifecycle.load_spec_revision(&uid).unwrap().unwrap().seq;
    lifecycle
        .append_spec_revision(&uid, sequence, draft)
        .unwrap();
    let mut workflow = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    crate::core::workflow::approve_feature(&store, &mut workflow, "CHG-983").unwrap();
    assert!(crate::core::workflow::feature_approved(
        &store, &workflow, "CHG-983"
    ));
    let current =
        crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(&store, &snapshot)
            .unwrap()
            .unwrap();
    assert_ne!(old.receipt_uid, current.receipt_uid);
    let review = crate::core::lifecycle::plan_snapshot::plan_review_receipt(&store, &snapshot)
        .unwrap()
        .unwrap();
    let refreshed = lifecycle
        .reconcile_reviewed_plan(
            &snapshot,
            &markdown,
            feature.readiness_evidence_refs.clone(),
            &current,
            &review,
        )
        .unwrap();
    assert_eq!(refreshed.decision_refs, feature.decision_refs);
    assert_eq!(
        serde_json::to_value(lifecycle.events_for_feature(&uid).unwrap()).unwrap(),
        serde_json::to_value(original_events).unwrap()
    );
    assert_eq!(
        crate::core::lifecycle::plan_snapshot::confirmed_product_spec_hash(&current),
        Some(
            crate::core::lifecycle::product_spec::current_product_hash(&store)
                .unwrap()
                .as_str()
        )
    );
}
