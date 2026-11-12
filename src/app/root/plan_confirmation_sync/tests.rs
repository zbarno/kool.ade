//! Integration-level proofs for the plan-confirmation sync seam: real
//! private-local store round trips, persistence across process reloads,
//! prompt-once evaluation requests, malformed-document containment, and
//! operator-cancellation protection. Pure reducer invariants are covered by
//! `core::planning_work::plan_confirmation` unit tests.

use super::sync;
use crate::app::session::Project;
use crate::app::session::test_support::{project_from, remove_planning_store_for};
use crate::core::lifecycle::plan_snapshot;
use crate::core::planning::{AcceptanceTestTrace, QuestionStore, ReadinessAxis};
use crate::core::planning_work::{Work, WorkKind, WorkStatus};

#[path = "tests/answer_reconciliation.rs"]
mod answer_reconciliation;
#[path = "tests/restart_stale_snapshot.rs"]
mod restart_stale_snapshot;
#[path = "tests/save_failure.rs"]
mod save_failure;

fn git(code_root: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-C"])
        .arg(code_root)
        .args(args)
        .output()
        .expect("git ran");
    assert!(out.status.success(), "git {args:?} failed");
}

struct Repo {
    _guard: tempfile::TempDir,
    root: std::path::PathBuf,
}

fn repo(tag: &str) -> Repo {
    let temp = tempfile::Builder::new()
        .prefix("koolade-pcsync-")
        .tempdir()
        .unwrap();
    let root = temp.path().join(tag);
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture User"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(root.join("README.md"), "fixture\n").unwrap();
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-qm", "fixture"]);
    Repo { _guard: temp, root }
}

fn feature_uid() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

fn spec_body(uid: &str) -> String {
    format!(
        "# FEAT-IT: Saved views\n\n<!-- koolade-artifact-id:v1 {{\"uid\":\"{uid}\",\"displayId\":\"FEAT-IT\",\"title\":\"Saved views\"}} -->\n\n## Intent\n\nAnalysts want reusable saved views so daily context is never lost.\n\n## Current Behavior\n\nViews cannot be saved.\n\n## Desired Behavior\n\nAnalysts can save, list, and reopen saved views.\n\n## Scope\n\n- Persist and restore named views\n- Out of scope: sharing views across users\n\n## Affected Product Areas\n\nView gallery and persistence.\n\n## Requirements\n\n- REQ-001: Saved views are restored after restart.\n\n## Decisions and Assumptions\n\n- Views are stored locally.\n\n## Acceptance Criteria\n\n1. A saved view can be reopened.\n2. The saved list survives relaunch.\n\n<!-- koolade-change:v1 {{\"schemaVersion\":1,\"uid\":\"{uid}\",\"displayId\":\"FEAT-IT\",\"status\":\"draft\"}} -->\n"
    )
}

fn intent_root(feature_uid: &str) -> Work {
    let mut root = Work::new(
        "task:intent-root-it".into(),
        "Saved views".into(),
        "Plan saved views".into(),
        "Starting".into(),
    );
    root.kind = WorkKind::Feature;
    root.feature_uid = Some(feature_uid.to_owned());
    root
}

/// Seed the feature into a freshly loaded project: document into
/// `active_features`, TaskIntent root into `planning_work`, optionally the
/// full readiness checklist proving Ready for the current spec revision.
fn seed(repo: &Repo, feature_uid: &str, with_readiness: bool) -> Project {
    let body = spec_body(feature_uid);
    let mut project = project_from(&repo.root);
    let store = &project.state.planning_store;
    let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
        .ok()
        .flatten()
        .expect("artifact identity parses");
    let contract = crate::core::workflow::feature_contract(&body);
    let snapshot =
        plan_snapshot::from_feature_with_author(store, "FEAT-IT", &identity, &body, &contract)
            .expect("snapshot builds");
    if with_readiness {
        let requirements = snapshot
            .requirements
            .iter()
            .map(|requirement| requirement.stable_id.clone())
            .collect::<Vec<_>>();
        let checklist = crate::core::planning::ReadinessChecklistV1 {
            feature_uid: feature_uid.to_owned(),
            plan_revision: 1,
            spec_revision: Some(snapshot.spec_revision.clone()),
            required_requirement_ids: Some(requirements.clone()),
            assessed_axes: all_material_axes(),
            acceptance_tests: requirements
                .iter()
                .map(|requirement| AcceptanceTestTrace {
                    requirement_id: requirement.clone(),
                    test_ref: format!("test:saved_views_restore_{requirement}"),
                })
                .collect(),
            evidence_refs: Vec::new(),
            unresolved_blocker_ids: Vec::new(),
            blocker_notes: Vec::new(),
            ready: true,
            stalled: false,
            no_progress_count: 0,
            progress_fingerprint: None,
            revision: 0,
        };
        QuestionStore::new(store)
            .save_readiness(&checklist, 0)
            .expect("readiness saves");
    }
    project.state.active_features = vec![("FEAT-IT".to_owned(), body)];
    project.planning_work.push(intent_root(feature_uid));
    project
}

/// All fourteen material axes a readiness assessment must cover.
fn all_material_axes() -> Vec<ReadinessAxis> {
    use ReadinessAxis::*;
    vec![
        Who,
        What,
        When,
        Where,
        Why,
        How,
        OutOfScope,
        CriticalPath,
        SystemBoundaries,
        FailureBehavior,
        DataOwnership,
        Authorization,
        MeasurableSuccess,
        TestCases,
    ]
}

fn the_card(project: &Project) -> &Work {
    project
        .planning_work
        .iter()
        .find(|work| work.kind == WorkKind::PlanConfirmation)
        .expect("exactly one canonical plan-confirmation card exists")
}

#[path = "tests/projection.rs"]
mod projection;

#[test]
fn cancelled_card_is_protected_from_resurrection() {
    let repo = repo("cancelled");
    let mut project = seed(&repo, &feature_uid(), true);
    let effects = sync(&mut project);
    assert!(
        effects
            .announcements
            .iter()
            .any(|text| text.contains("review")),
        "creation is announced to the user"
    );
    let card_uid = the_card(&project).uid.clone();
    let planning_id = crate::persistence::cancelled_work::planning_id(&card_uid);
    project.cancelled_work.insert(planning_id);
    let before = project.planning_work.clone();
    let quiet = sync(&mut project);
    assert_eq!(
        project.planning_work, before,
        "operator cancellation wins over reconcile"
    );
    assert!(quiet.announcements.is_empty());
    assert!(quiet.evaluations_wanted.is_empty());
    assert_eq!(the_card(&project).uid, card_uid);
    remove_planning_store_for(&repo.root);
}

#[test]
/// Production boundary: post-answer adoption may reconcile and announce a
/// not-ready plan before the periodic scheduler owns dispatch. Persisted state
/// must leave the prompt unstamped so the later idle sweep can claim and return
/// one evaluation request.
/// A pure reducer test cannot prove the app seam persists this distinction.
/// Uses the real private store and a temporary local Git project; no external
/// services, standard fixture setup cost.
fn announcement_only_sync_leaves_readiness_prompt_unclaimed_for_the_idle_sweep() {
    let repo = repo("deferred-evaluation");
    let uid = feature_uid();
    let mut project = seed(&repo, &uid, false);

    let announced = super::sync_for_announcement(&mut project);
    assert!(announced.evaluations_wanted.is_empty());
    let payload = the_card(&project).plan_confirmation.as_ref().unwrap();
    assert_eq!(payload.evaluation_prompted_for, None);

    let mut reloaded = project_from(&repo.root);
    reloaded.planning_work = crate::core::planning_work::load(&reloaded.state.planning_store)
        .expect("announcement-only state reloads");
    let idle = sync(&mut reloaded);
    assert_eq!(idle.evaluations_wanted.len(), 1);
    assert_eq!(idle.evaluations_wanted[0].work_key, the_card(&reloaded).key);
    assert_eq!(
        the_card(&reloaded)
            .plan_confirmation
            .as_ref()
            .unwrap()
            .evaluation_prompted_for
            .as_deref(),
        Some(
            the_card(&reloaded)
                .plan_confirmation
                .as_ref()
                .unwrap()
                .snapshot_spec_revision
                .as_str()
        )
    );
    assert!(sync(&mut reloaded).evaluations_wanted.is_empty());
    remove_planning_store_for(&repo.root);
}
