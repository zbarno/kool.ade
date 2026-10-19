use super::*;

#[test]
// Production boundary: conflicting specification markers must abort the complete
// question/readiness/spec transaction. Unit tests cannot prove that these persisted
// records remain unchanged together. Dependencies: temporary managed-local store;
// no services. Setup cost: low, one synthetic feature and question.
fn conflicting_spec_marker_does_not_resolve_question_or_update_readiness() {
    use crate::{
        artifacts::planning_store::{PlanningStore, StoreMode},
        core::planning::{QuestionCategory, QuestionStatus, QuestionStore},
        harness::prompt_catalog::TechnicalResolutionV1,
    };

    let root = std::env::temp_dir().join(format!("decision-conflict-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (feature_uid, spec_revision) = install_feature(&store);
    persist(
        &store,
        &feature_uid,
        1,
        &spec_revision,
        planner_output(&feature_uid),
    )
    .unwrap();

    let questions = QuestionStore::new(&store);
    let initial_question = questions.list().unwrap().remove(0);
    let (mut question, record_revision) = questions.load(&initial_question.uid).unwrap();
    question.category = QuestionCategory::Api;
    question.revision += 1;
    questions.save(&question, record_revision).unwrap();
    let (feature_id, mut markdown) =
        crate::artifacts::product_docs::active_features(&store).remove(0);
    let marker = format!("<!-- koolade-clarification-resolution:{} -->", question.uid);
    markdown.push_str(&format!("\n\n## Decisions and Assumptions\n\n{marker}\n"));
    let path =
        crate::artifacts::product_docs::document_path(&store, &format!("feature:{feature_id}"))
            .unwrap();
    let relative = path
        .strip_prefix(&store.root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let revision = store.revision().unwrap();
    store
        .transaction(&[(relative, markdown.as_bytes().to_vec())], Some(&revision))
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let expected_spec_revision = crate::core::lifecycle::PlanSnapshotV1::from_feature(
        &feature_id,
        &identity,
        &markdown,
        &crate::core::workflow::feature_contract(&markdown),
    )
    .unwrap()
    .spec_revision;
    let (before_readiness, _) = questions.load_readiness(&feature_uid).unwrap().unwrap();
    let before_store_revision = store.revision().unwrap();
    let mut output = planner_output(&feature_uid);
    output.proposed_questions.clear();
    output.resolved_questions = vec![TechnicalResolutionV1 {
        question_uid: question.uid.clone(),
        decision: "Keep the established API".into(),
        rationale: "Repository precedent supports it.".into(),
        selected_option_id: None,
        evidence_refs: vec![format!("repository-overview:{feature_uid}")],
    }];

    assert!(persist(&store, &feature_uid, 1, &expected_spec_revision, output).is_err());
    let (after_question, _) = questions.load(&question.uid).unwrap();
    let (after_readiness, _) = questions.load_readiness(&feature_uid).unwrap().unwrap();
    assert_eq!(after_question.status, QuestionStatus::Open);
    assert_eq!(after_readiness, before_readiness);
    assert_eq!(store.revision().unwrap(), before_store_revision);
    let (_, persisted_markdown) = crate::artifacts::product_docs::active_features(&store)
        .into_iter()
        .find(|(id, _)| id == &feature_id)
        .unwrap();
    assert_eq!(persisted_markdown, markdown);
    let _ = std::fs::remove_dir_all(root);
}
