use super::*;
use crate::domain::ArtifactIdentity;

fn spec() -> String {
    "# CHG-021: Report exports\n\n## Intent\n\nHelp operators export reports.\n\n## Current Behavior\n\nReports cannot be exported.\n\n## Desired Behavior\n\nOperators can download reports.\n\n## Scope\n\n- CSV export\n- Out of scope: scheduled delivery\n\n## Affected Product Areas\n\nReporting and downloads.\n\n## Requirements\n\n- REQ-001: Exports include selected rows. Rationale: operators should exclude irrelevant data. ACs: AC-001\n- Keep the current filename behavior.\n\n## Decisions and Assumptions\n\n- CSV is sufficient for this release.\n\n## Acceptance Criteria\n\n1. Selected rows are exported.\n2. Empty results produce a valid file.\n".into()
}

fn identified(markdown: &str) -> (ArtifactIdentity, String) {
    let identified =
        ArtifactIdentity::preserve_markdown(markdown, None, "CHG-021", "Report exports").unwrap();
    let identity = ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    (identity, identified)
}

fn plan_receipt(
    snapshot: &PlanSnapshotV1,
    decision: crate::core::lifecycle::GateDecision,
) -> crate::core::lifecycle::GateReceipt {
    use crate::core::lifecycle::{GateActor, GateReceipt};
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};
    let author_run = snapshot
        .author_run_id
        .clone()
        .expect("test review plan has explicit author provenance");
    let author_session = snapshot
        .author_session_id
        .clone()
        .expect("test review plan has explicit author session provenance");
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let manifest = render(&RoleInputV1 {
        role: PromptRole::PlanReviewer,
        workspace_uid: "fixture-workspace".into(),
        feature_uid: snapshot.feature_uid.clone(),
        task_uid: None,
        original_goal: "Adversarially review the user-approved plan".into(),
        approved_spec_ref: Some(snapshot.uid.clone()),
        approved_spec_hash: Some(snapshot.content_hash.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "plan-review/v1".into(),
        output_schema_version: PromptRole::PlanReviewer.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: snapshot.spec_revision.clone(),
            subject_hash: snapshot.content_hash.clone(),
            author_run_id: author_run.clone(),
            reviewer_run_id: reviewer_run.clone(),
            author_session_id: Some(author_session.clone()),
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000006".into()),
            requirements_matrix: snapshot
                .requirements
                .iter()
                .map(|req| req.text.clone())
                .collect(),
            explicit_exclusions: snapshot.non_goals.clone(),
        },
    })
    .unwrap()
    .manifest;
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::IndependentReviewer,
        decision,
        recorded_at_unix_ms: 1,
        evidence_refs: std::iter::once(format!("plan:{}", snapshot.uid))
            .chain(
                snapshot
                    .requirements
                    .iter()
                    .map(|requirement| format!("requirement:{}", requirement.stable_id)),
            )
            .chain(std::iter::once("plan-review/fixture".into()))
            .collect(),
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run),
        author_run_id: Some(author_run),
        reviewer_session_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        author_session_id: Some(author_session),
    }
}

fn pass_verdict(snapshot: &PlanSnapshotV1) -> crate::harness::prompt_catalog::ReviewVerdictV1 {
    crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        pass: true,
        coverage: snapshot
            .requirements
            .iter()
            .map(|requirement| crate::harness::prompt_catalog::CoverageV1 {
                requirement: requirement.stable_id.clone(),
                status: crate::harness::prompt_catalog::CoverageStatus::Pass,
                evidence: vec![format!("requirement:{}", requirement.stable_id)],
            })
            .collect(),
        findings: Vec::new(),
        untested: Vec::new(),
    }
}

fn persist_review(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &crate::core::lifecycle::GateReceipt,
) {
    let passed = receipt.decision == crate::core::lifecycle::GateDecision::Approved;
    let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        pass: passed,
        coverage: snapshot
            .requirements
            .iter()
            .enumerate()
            .map(
                |(index, requirement)| crate::harness::prompt_catalog::CoverageV1 {
                    requirement: requirement.stable_id.clone(),
                    status: if passed {
                        crate::harness::prompt_catalog::CoverageStatus::Pass
                    } else if index == 0 {
                        crate::harness::prompt_catalog::CoverageStatus::Fail
                    } else {
                        crate::harness::prompt_catalog::CoverageStatus::Untested
                    },
                    evidence: vec![format!("requirement:{}", requirement.stable_id)],
                },
            )
            .collect(),
        findings: if passed {
            Vec::new()
        } else {
            vec![crate::harness::prompt_catalog::ReviewFindingV1 {
                id: "F-TEST".into(),
                requirement: snapshot.requirements[0].stable_id.clone(),
                severity: crate::harness::prompt_catalog::Severity::Medium,
                kind: crate::harness::prompt_catalog::FindingKind::Requirements,
                location: snapshot.requirements[0].stable_id.clone(),
                evidence: vec![format!(
                    "requirement:{}",
                    snapshot.requirements[0].stable_id
                )],
                repro: Some("The requirement can be interpreted in two ways.".into()),
                proposed_correction: "Clarify the requirement.".into(),
            }]
        },
        untested: Vec::new(),
    };
    let (path, bytes, check) = review_change(store, snapshot, receipt, &verdict)
        .unwrap()
        .unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();
}

fn persist_snapshot(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) {
    let (path, bytes, check) = record_change(store, snapshot).unwrap().unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();
}

fn persist_user_confirmation(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) {
    use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};
    let product_hash = crate::core::lifecycle::product_spec::current_product_hash(store).unwrap();
    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![
            format!("plan:{}", snapshot.uid),
            format!("product-spec:{product_hash}"),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let (path, bytes, check) =
        crate::core::lifecycle::plan_snapshot::user_confirmation_change(store, snapshot, &receipt)
            .unwrap()
            .unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();
}

#[test]
fn snapshot_is_stable_content_addressed_and_detects_edits() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let first = PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    let repeated =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    assert_eq!(first, repeated);
    assert_eq!(first.schema_version, 2);
    assert_eq!(first.requirements[0].stable_id, "REQ-001");
    assert_eq!(first.requirements[1].stable_id.len(), 16);
    assert_eq!(
        first.requirements[0].rationale.as_deref(),
        Some("operators should exclude irrelevant data.")
    );
    assert_eq!(first.requirements[0].ac_ids, ["AC-001"]);
    assert!(first.requirements[1].rationale.is_none());
    assert!(first.requirements[1].ac_ids.is_empty());
    assert!(first.acceptance_criteria[0].starts_with("AC-001:"));
    assert_eq!(first.non_goals, ["Out of scope: scheduled delivery"]);

    let changed = markdown.replace("valid file", "valid CSV file");
    let changed_contract = crate::core::workflow::feature_contract(&changed);
    let changed_snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &changed, &changed_contract).unwrap();
    assert_ne!(first.content_hash, changed_snapshot.content_hash);
    assert_ne!(first.uid, changed_snapshot.uid);

    let mut corrupt = first;
    corrupt.outcome.push_str(" changed");
    assert!(corrupt.validate().is_err());
}

#[test]
fn empty_explicit_requirement_id_uses_a_deterministic_generated_id() {
    let malformed = spec().replace("REQ-001: Exports", "REQ-: Exports");
    let (identity, markdown) = identified(&malformed);
    let contract = crate::core::workflow::feature_contract(&markdown);
    let first = PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    let again = PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    assert_ne!(first.requirements[0].stable_id, "REQ-");
    assert!(first.requirements[0].stable_id.starts_with("REQ-"));
    assert_eq!(first.requirements[0].stable_id.len(), 16);
    assert_eq!(
        first.requirements[0].stable_id,
        again.requirements[0].stable_id
    );
}

#[test]
fn snapshot_includes_explicitly_deferred_question_assumptions() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::core::planning::{
        DeferredAssumption, QuestionCategory, QuestionOption, QuestionPriority, QuestionStatus,
        QuestionStore, QuestionV1,
    };
    let root = std::env::temp_dir().join(format!("plan-deferred-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let question = QuestionV1 {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        feature_uid: identity.uid.clone(),
        plan_revision: 1,
        decision_subject: "report ownership".into(),
        decision_domain: "data".into(),
        origin_requirement_ids: vec!["REQ-001".into()],
        category: QuestionCategory::Data,
        priority: QuestionPriority::Optional,
        question: "Who owns archived exports?".into(),
        reason: "Retention ownership is not needed for the first release.".into(),
        options: vec![QuestionOption {
            id: "operator".into(),
            label: "The operator".into(),
            tradeoffs: "Requires later confirmation.".into(),
        }],
        recommended_option_id: None,
        no_answer_impact: "The ownership policy remains open.".into(),
        evidence_refs: vec!["plan:fixture".into()],
        assumption_if_deferred: Some("The operator owns exports.".into()),
        deferred: Some(DeferredAssumption {
            assumption: "The operator owns exports.".into(),
            owner: "Workspace administrator".into(),
            risk: "Retention may differ by workspace.".into(),
            verification_test: "Verify ownership policy with an administrator.".into(),
            follow_up_task: "Confirm export retention ownership.".into(),
        }),
        depends_on_question_ids: Vec::new(),
        status: QuestionStatus::Deferred,
        answer_text: None,
        evidence_resolution: None,
        selected_option: None,
        answered_by: None,
        answered_at_ms: None,
        deferred_by: Some("operator".into()),
        deferred_at_ms: Some(1),
        revision: 0,
    };
    QuestionStore::new(&store).save(&question, 0).unwrap();

    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot =
        super::from_feature_with_author(&store, "CHG-021", &identity, &markdown, &contract)
            .unwrap();
    assert_eq!(snapshot.deferred_assumptions.len(), 1);
    assert!(snapshot.deferred_assumptions[0].contains("The operator owns exports."));
    assert!(snapshot.deferred_assumptions[0].contains("Workspace administrator"));
    assert!(snapshot.validate().is_ok());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn legacy_snapshot_acceptance_criteria_remain_valid_without_ids() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut legacy =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    legacy.schema_version = 1;
    legacy.acceptance_criteria = vec!["Selected rows are exported.".into()];
    legacy.requirements[0].rationale = None;
    legacy.requirements[0].ac_ids.clear();
    legacy.content_hash = legacy.compute_content_hash().unwrap();
    legacy.uid = crate::core::lifecycle::plan_snapshot::content::snapshot_uid(
        &legacy.feature_uid,
        &legacy.content_hash,
    )
    .unwrap();
    legacy.validate().unwrap();
}

#[test]
fn serialized_legacy_snapshot_loads_with_its_original_hash_and_uid() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("legacy-plan-snapshot-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.schema_version = 1;
    snapshot.acceptance_criteria = vec!["Selected rows are exported.".into()];
    for requirement in &mut snapshot.requirements {
        requirement.rationale = None;
        requirement.ac_ids.clear();
    }
    snapshot.content_hash = snapshot.compute_content_hash().unwrap();
    snapshot.uid = crate::core::lifecycle::plan_snapshot::content::snapshot_uid(
        &snapshot.feature_uid,
        &snapshot.content_hash,
    )
    .unwrap();
    let path = format!("state/lifecycle_plans/{}.json", snapshot.uid);
    let record = serde_json::json!({
        "schemaVersion": 1,
        "uid": snapshot.uid,
        "createdAtMs": 1,
        "updatedAtMs": 1,
        "revision": 1,
        "data": snapshot.clone(),
    });
    store
        .transaction(&[(path, serde_json::to_vec(&record).unwrap())], None)
        .unwrap();
    let loaded = crate::core::lifecycle::plan_snapshot::load_persisted(&store, &snapshot.uid)
        .unwrap()
        .unwrap();
    assert_eq!(loaded, snapshot);
}

#[test]
fn snapshot_rejects_malformed_acceptance_criterion_ids() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.acceptance_criteria[0] = "AC-xyz: Selected rows are exported.".into();
    snapshot.content_hash = snapshot.compute_content_hash().unwrap();
    snapshot.uid = crate::core::lifecycle::plan_snapshot::content::snapshot_uid(
        &snapshot.feature_uid,
        &snapshot.content_hash,
    )
    .unwrap();
    assert!(snapshot.validate().is_err());
}

#[test]
fn snapshot_record_is_create_only_and_survives_reopen() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("plan-snapshot-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    let (path, bytes, check) = record_change(&store, &snapshot).unwrap().unwrap();
    store
        .transaction_with_record_revisions(&[(path.clone(), bytes)], &[check])
        .unwrap();
    assert!(is_persisted(&store, &snapshot).unwrap());
    assert!(record_change(&store, &snapshot).unwrap().is_none());
    let reopened = PlanningStore::new(
        store.project_id,
        store.root.clone(),
        StoreMode::ManagedLocal,
    );
    assert!(is_persisted(&reopened, &snapshot).unwrap());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn user_confirmation_is_persisted_once_and_bound_to_the_exact_snapshot() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};
    let root = std::env::temp_dir().join(format!("plan-confirmation-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&root, "Plan Confirmation Fixture").unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join(".koolade-packet"),
        StoreMode::ManagedLocal,
    );
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    persist_snapshot(&store, &snapshot);
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &snapshot).unwrap()
    );

    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![
            format!("plan:{}", snapshot.uid),
            format!(
                "product-spec:{}",
                crate::core::lifecycle::product_spec::current_product_hash(&store).unwrap()
            ),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let mut forged = receipt.clone();
    forged.subject_sha256 = "b".repeat(64);
    assert!(user_confirmation_change(&store, &snapshot, &forged).is_err());
    let (path, bytes, check) = user_confirmation_change(&store, &snapshot, &receipt)
        .unwrap()
        .unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();
    assert!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &snapshot).unwrap()
    );
    assert!(
        user_confirmation_change(&store, &snapshot, &receipt)
            .unwrap()
            .is_none()
    );
    let reopened = PlanningStore::new(
        store.project_id,
        store.root.clone(),
        StoreMode::ManagedLocal,
    );
    assert!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(&reopened, &snapshot).unwrap()
    );

    let changed = markdown.replace("valid file", "valid CSV file");
    let changed_contract = crate::core::workflow::feature_contract(&changed);
    let changed_snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &changed, &changed_contract).unwrap();
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &changed_snapshot)
            .unwrap()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
// Production boundary: confirmation lookup across immutable candidate records
// must distinguish semantic edits from formatting-only edits and fail closed.
// A receipt-only unit test cannot prove candidate identity binding. Local
// temporary PlanningStore only; no external services; low setup cost.
fn material_and_editorial_candidate_diffs_do_not_reuse_confirmation() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("plan-diff-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&root, "Plan diff fixture").unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join(crate::artifacts::layout::canonical::ROOT),
        StoreMode::ManagedLocal,
    );
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let confirmed =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    persist_snapshot(&store, &confirmed);
    persist_user_confirmation(&store, &confirmed);

    let material_markdown = markdown.replace("download reports", "export and retain reports");
    let material_contract = crate::core::workflow::feature_contract(&material_markdown);
    let material =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &material_markdown, &material_contract)
            .unwrap();
    let editorial_markdown = markdown.replace(
        "Help operators export reports.",
        "Help operators  export reports.",
    );
    let editorial_contract = crate::core::workflow::feature_contract(&editorial_markdown);
    let editorial = PlanSnapshotV1::from_feature(
        "CHG-021",
        &identity,
        &editorial_markdown,
        &editorial_contract,
    )
    .unwrap();

    assert_ne!(material.content_hash, confirmed.content_hash);
    assert_ne!(editorial.content_hash, confirmed.content_hash);
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &material).unwrap()
    );
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &editorial).unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn task_gate_requires_a_current_independent_approved_plan_review() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::core::lifecycle::GateDecision;
    let root = std::env::temp_dir().join(format!("plan-review-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&root, "Plan review fixture").unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join(crate::artifacts::layout::canonical::ROOT),
        StoreMode::ManagedLocal,
    );
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.author_run_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    snapshot.author_session_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    persist_snapshot(&store, &snapshot);
    assert!(
        !has_approved_review(&store, &snapshot).unwrap(),
        "missing reviewer must block"
    );

    let unconfirmed = plan_receipt(&snapshot, GateDecision::Approved);
    assert!(
        review_change(&store, &snapshot, &unconfirmed, &pass_verdict(&snapshot)).is_err(),
        "independent review cannot be recorded before explicit user confirmation"
    );
    persist_user_confirmation(&store, &snapshot);

    let stale = plan_receipt(&snapshot, GateDecision::Approved);
    let mut stale = stale;
    stale.subject_sha256 = "b".repeat(64);
    assert!(
        review_change(&store, &snapshot, &stale, &pass_verdict(&snapshot)).is_err(),
        "stale plan hash must be rejected"
    );

    let mut same_author = plan_receipt(&snapshot, GateDecision::Approved);
    same_author.reviewer_run_id = same_author.author_run_id.clone();
    assert!(
        review_change(&store, &snapshot, &same_author, &pass_verdict(&snapshot)).is_err(),
        "same author cannot review"
    );

    let mut forged_author = plan_receipt(&snapshot, GateDecision::Approved);
    forged_author.author_run_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    assert!(
        review_change(&store, &snapshot, &forged_author, &pass_verdict(&snapshot)).is_err(),
        "review receipt must retain the author run recorded in the snapshot"
    );
    let mut forged_author_session = plan_receipt(&snapshot, GateDecision::Approved);
    forged_author_session.author_session_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    assert!(
        review_change(
            &store,
            &snapshot,
            &forged_author_session,
            &pass_verdict(&snapshot)
        )
        .is_err(),
        "review receipt must retain the author session recorded in the snapshot"
    );

    let rejected = plan_receipt(&snapshot, GateDecision::Rejected);
    persist_review(&store, &snapshot, &rejected);
    assert!(
        !has_approved_review(&store, &snapshot).unwrap(),
        "rejected review must block"
    );
    let rejected_verdict =
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(&store, &snapshot)
            .unwrap()
            .expect("blocking findings are retained with the review receipt");
    assert_eq!(rejected_verdict.findings[0].id, "F-TEST");
    assert_eq!(rejected_verdict.findings[0].location, "REQ-001");
    assert!(rejected_verdict.findings[0].repro.is_some());

    let next_root =
        std::env::temp_dir().join(format!("plan-review-approved-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&next_root, "Plan review fixture").unwrap();
    let approved_store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        next_root.join(crate::artifacts::layout::canonical::ROOT),
        StoreMode::ManagedLocal,
    );
    persist_snapshot(&approved_store, &snapshot);
    persist_user_confirmation(&approved_store, &snapshot);
    let approved = plan_receipt(&snapshot, GateDecision::Approved);
    persist_review(&approved_store, &snapshot, &approved);
    assert!(has_approved_review(&approved_store, &snapshot).unwrap());
    let persisted_verdict =
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(&approved_store, &snapshot)
            .unwrap()
            .expect("review verdict is persisted with its receipt");
    assert_eq!(persisted_verdict, pass_verdict(&snapshot));
    let reopened = PlanningStore::new(
        approved_store.project_id,
        approved_store.root.clone(),
        StoreMode::ManagedLocal,
    );
    assert_eq!(
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(&reopened, &snapshot).unwrap(),
        Some(persisted_verdict.clone()),
        "the sanitized structured verdict survives app restart and shared-store reload"
    );
    // A saved verdict may remain after a product-spec edit, but replay must
    // require fresh consent for the current product context.
    let docs = crate::artifacts::product_docs::load_documents(&reopened)
        .unwrap()
        .unwrap();
    let doc = &docs[0];
    let relative = reopened
        .layout()
        .product_root()
        .join(&doc.module.path)
        .strip_prefix(reopened.layout().root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    reopened
        .transaction(
            &[(
                relative,
                format!("{}\nUpdated product context.\n", doc.content).into_bytes(),
            )],
            None,
        )
        .unwrap();
    assert!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(&reopened, &snapshot).unwrap()
    );
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(&reopened, &snapshot)
            .unwrap()
    );
    assert_eq!(
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(&reopened, &snapshot).unwrap(),
        Some(persisted_verdict),
        "stale persisted verdict cannot be treated as current approval after restart"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(next_root);
}

#[test]
fn legacy_review_receipt_can_be_replaced_by_a_verdict_backed_review() {
    use crate::artifacts::planning_store::{PlanningStore, RecordRevisionCheck, StoreMode};
    use crate::core::lifecycle::GateDecision;
    let root = std::env::temp_dir().join(format!("legacy-plan-review-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&root, "Plan review fixture").unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join(crate::artifacts::layout::canonical::ROOT),
        StoreMode::ManagedLocal,
    );
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.author_run_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    snapshot.author_session_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    persist_snapshot(&store, &snapshot);
    persist_user_confirmation(&store, &snapshot);

    let legacy_receipt = plan_receipt(&snapshot, GateDecision::Approved);
    let path = format!("state/lifecycle_plan_reviews/{}.json", snapshot.uid);
    let legacy_record = serde_json::json!({
        "schemaVersion": 1,
        "uid": snapshot.uid.clone(),
        "createdAtMs": 1,
        "updatedAtMs": 1,
        "revision": 1,
        "receipt": legacy_receipt,
    });
    store
        .transaction_with_record_revisions(
            &[(
                path.clone(),
                serde_json::to_vec_pretty(&legacy_record).unwrap(),
            )],
            &[RecordRevisionCheck {
                path: path.clone(),
                expected_revision: 0,
            }],
        )
        .unwrap();
    assert!(!has_approved_review(&store, &snapshot).unwrap());

    let current_receipt = plan_receipt(&snapshot, GateDecision::Approved);
    let verdict = pass_verdict(&snapshot);
    let (updated_path, bytes, check) = review_change(&store, &snapshot, &current_receipt, &verdict)
        .unwrap()
        .expect("legacy evidence requires a real new review");
    assert_eq!(updated_path, path);
    assert_eq!(check.expected_revision, 1);
    store
        .transaction_with_record_revisions(&[(updated_path, bytes)], &[check])
        .unwrap();
    assert!(has_approved_review(&store, &snapshot).unwrap());
    assert_eq!(
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(&store, &snapshot).unwrap(),
        Some(verdict)
    );
    let _ = std::fs::remove_dir_all(root);
}
