//! Unit-level proofs for the canonical plan-confirmation reconciler:
//! idempotence, duplicate collapse, exclusion, multi-feature independence,
//! prompt-once semantics, and lifecycle-status protection. End-to-end flows
//! (real assessment turns, approvals, drift) live in app-level tests.

use super::*;
use std::path::PathBuf;

fn feature_uid() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

struct Fixture {
    _code: tempfile::TempDir,
    code_root: PathBuf,
    state: PlannerState,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let code_root = temp.path().join("code");
    std::fs::create_dir_all(&code_root).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(["-C"])
            .arg(&code_root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Fixture User"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(code_root.join("README.md"), "fixture\n").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-qm", "fixture"]);
    let project = crate::app::session::test_support::project_from(&code_root);
    Fixture {
        _code: temp,
        code_root,
        state: project.state,
    }
}

fn draft_body(uid: &str) -> String {
    format!(
        "# FEAT-TEST: Saved views\n\n<!-- koolade-artifact-id:v1 {{\"uid\":\"{uid}\",\"displayId\":\"FEAT-TEST\",\"title\":\"Saved views\"}} -->\n\n## Intent\n\nAnalysts want reusable saved views so daily context is never lost.\n\n## Current Behavior\n\nViews cannot be saved.\n\n## Desired Behavior\n\nAnalysts can save, list, and reopen saved views.\n\n## Scope\n\n- Persist and restore named views\n- Out of scope: sharing views across users\n\n## Affected Product Areas\n\nView gallery and persistence.\n\n## Requirements\n\n- REQ-001: Saved views are restored after restart.\n\n## Decisions and Assumptions\n\n- Views are stored locally.\n\n## Acceptance Criteria\n\n1. A saved view can be reopened.\n2. The saved list survives relaunch.\n\n<!-- koolade-change:v1 {{\"schemaVersion\":1,\"uid\":\"{uid}\",\"displayId\":\"CHG-TEST\",\"status\":\"draft\"}} -->\n"
    )
}

fn intent_root(feature_uid: &str) -> Work {
    let mut root = Work::new(
        "task:intent-root".into(),
        "Saved views".into(),
        "Plan saved views".into(),
        "Starting".into(),
    );
    root.kind = WorkKind::Feature;
    root.feature_uid = Some(feature_uid.to_owned());
    root
}

fn with_feature(fixture: &Fixture, feature_id: &str, body: &str) -> PlannerState {
    let mut state = fixture.state.clone();
    state.active_features = vec![(feature_id.to_owned(), body.to_owned())];
    state
}

fn run(state: &PlannerState, work: &mut Vec<Work>) -> anyhow::Result<PlanConfirmationReport> {
    super::reconcile(state, work, &std::collections::BTreeSet::new())
}

#[test]
fn unanchored_features_create_no_card() {
    let fx = fixture();
    let state = with_feature(&fx, "F1", &draft_body(&feature_uid()));
    let mut work = Vec::new();
    let report = run(&state, &mut work).unwrap();
    assert!(!report.changed);
    assert!(work.is_empty());
}

#[test]
fn unready_plan_creates_one_needs_attention_card_and_is_idempotent() {
    let uid = feature_uid();
    let fx = fixture();
    let state = with_feature(&fx, "F1", &draft_body(&uid));
    let mut work = vec![intent_root(&uid)];

    let first = run(&state, &mut work).unwrap();
    assert!(first.changed);
    assert_eq!(work.len(), 2, "exactly one canonical card per feature");
    let card = work
        .iter()
        .find(|item| item.kind == WorkKind::PlanConfirmation)
        .expect("card");
    assert_eq!(card.status, WorkStatus::NeedsAttention);
    assert!(card.detail.contains("not ready for approval"));
    assert_eq!(
        card.plan_confirmation.as_ref().unwrap().cause,
        Some(PlanNotReadyCause::AssessmentNeeded)
    );
    assert_eq!(first.evaluations_wanted.len(), 1);
    assert_eq!(first.announcements.len(), 1);

    // Second identical pass: strictly a no-op, no re-notification, no second
    // evaluation prompt.
    let snapshot_before = work.clone();
    let second = run(&state, &mut work).unwrap();
    assert!(!second.changed, "steady state must not churn the record");
    assert!(second.evaluations_wanted.is_empty());
    assert!(second.announcements.is_empty());
    assert_eq!(work, snapshot_before);
}

#[test]
fn malformed_document_blocks_without_evaluating() {
    let uid = feature_uid();
    let body = format!(
        "No heading, only a marker.\n\n<!-- koolade-change:v1 {{\"schemaVersion\":1,\"uid\":\"{uid}\",\"displayId\":\"CHG-BAD\",\"status\":\"draft\"}} -->\n"
    );
    let fx = fixture();
    let state = with_feature(&fx, "F1", &body);
    let mut work = vec![intent_root(&uid)];
    let report = run(&state, &mut work).unwrap();
    let card = work
        .iter()
        .find(|item| item.kind == WorkKind::PlanConfirmation)
        .expect("card");
    assert_eq!(card.status, WorkStatus::NeedsAttention);
    assert_eq!(
        card.plan_confirmation.as_ref().unwrap().cause,
        Some(PlanNotReadyCause::MalformedDocument)
    );
    assert!(
        report.evaluations_wanted.is_empty(),
        "malformed documents must not burn evaluation turns"
    );
}

#[test]
fn duplicate_canonical_cards_collapse_to_one() {
    let uid = feature_uid();
    let fx = fixture();
    let state = with_feature(&fx, "F1", &draft_body(&uid));
    let mut work = vec![intent_root(&uid)];
    run(&state, &mut work).unwrap();
    let keeper = work
        .iter()
        .find(|item| item.kind == WorkKind::PlanConfirmation)
        .expect("card")
        .uid
        .clone();
    let stray_source = work
        .iter()
        .find(|item| item.kind == WorkKind::PlanConfirmation)
        .expect("card")
        .clone();
    let mut stray = stray_source;
    stray.uid = uuid::Uuid::new_v4().hyphenated().to_string();
    stray.key = "plan-confirmation-stray".into();
    work.push(stray);
    assert_eq!(work.len(), 3);

    let report = run(&state, &mut work).unwrap();
    assert!(report.changed);
    assert_eq!(report.duplicates_removed, 1);
    let survivors: Vec<_> = work
        .iter()
        .filter(|item| item.kind == WorkKind::PlanConfirmation)
        .collect();
    assert_eq!(survivors.len(), 1);
    assert_eq!(survivors[0].uid, keeper, "best-evidenced card survives");
}

#[test]
fn excluded_key_is_left_alone() {
    let uid = feature_uid();
    let fx = fixture();
    let state = with_feature(&fx, "F1", &draft_body(&uid));
    let mut work = vec![intent_root(&uid)];
    run(&state, &mut work).unwrap();
    let key = work
        .iter()
        .find(|item| item.kind == WorkKind::PlanConfirmation)
        .expect("card")
        .key
        .clone();
    let before = work.clone();

    let mut excluded = std::collections::BTreeSet::new();
    excluded.insert(key);
    let report = super::reconcile(&state, &mut work, &excluded).unwrap();
    assert!(!report.changed);
    assert_eq!(work, before);
}

#[test]
fn two_features_get_independent_cards() {
    let uid_a = feature_uid();
    let uid_b = feature_uid();
    let fx = fixture();
    let mut state = with_feature(&fx, "FA", &draft_body(&uid_a));
    state
        .active_features
        .push(("FB".into(), draft_body(&uid_b)));
    let mut work = vec![intent_root(&uid_a), intent_root(&uid_b)];

    let report = run(&state, &mut work).unwrap();
    assert!(report.changed);
    let cards: Vec<_> = work
        .iter()
        .filter(|item| item.kind == WorkKind::PlanConfirmation)
        .collect();
    assert_eq!(cards.len(), 2);
    assert_ne!(cards[0].key, cards[1].key);
    let linked: Vec<String> = cards
        .iter()
        .filter_map(|card| card.feature_uid.clone())
        .collect();
    assert!(linked.contains(&uid_a));
    assert!(linked.contains(&uid_b));
}

#[test]
fn in_progress_cards_are_protected_from_generic_lifecycle_demotion() {
    use crate::core::planning_work::reconcile_inactive;
    let uid = feature_uid();
    let fx = fixture();
    let state = with_feature(&fx, "F1", &draft_body(&uid));
    let mut work = vec![intent_root(&uid)];
    run(&state, &mut work).unwrap();
    let card = work
        .iter_mut()
        .find(|item| item.kind == WorkKind::PlanConfirmation)
        .expect("card");
    card.status = WorkStatus::InProgress; // e.g. review running
    let before = card.status;

    reconcile_inactive(&state, &mut work, Some("some-other-turn:"));

    assert_eq!(
        work.iter()
            .find(|item| item.kind == WorkKind::PlanConfirmation)
            .unwrap()
            .status,
        before,
        "generic inactive reconciliation must never demote canonical cards"
    );
}

#[test]
fn payload_serde_roundtrip_is_closed() {
    let base = PlanConfirmationV1 {
        schema_version: PLAN_CONFIRMATION_SCHEMA,
        feature_uid: feature_uid(),
        snapshot_content_hash: "b".repeat(64),
        snapshot_spec_revision: "c".repeat(64),
        pin_is_published: false,
        ready: true,
        cause: None,
        blockers: Vec::new(),
        evaluation_prompted_for: None,
        announced_ready: None,
        evaluated_at_unix_ms: 1,
    };
    let parsed: PlanConfirmationV1 =
        serde_json::from_slice(&serde_json::to_vec(&base).unwrap()).unwrap();
    assert_eq!(parsed, base);
    assert!(parsed.validate().is_ok());
    // Closed schema: unknown fields must be rejected on deserialization.
    let encoded = serde_json::to_string(&base).unwrap();
    let mut tampered = String::from("{\"rogueField\":true,");
    tampered.push_str(&encoded[1..]);
    assert!(serde_json::from_str::<PlanConfirmationV1>(&tampered).is_err());
    // Semantic consistency: a "ready" payload cannot carry blockers.
    let broken = PlanConfirmationV1 {
        blockers: vec!["still open".into()],
        ..base.clone()
    };
    assert!(broken.validate().is_err());
    let blocked = PlanConfirmationV1 {
        ready: false,
        cause: Some(PlanNotReadyCause::AssessmentNeeded),
        blockers: vec!["unfinished".into()],
        ..base
    };
    assert!(blocked.validate().is_ok());
}
