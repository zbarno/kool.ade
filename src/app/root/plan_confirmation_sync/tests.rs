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
        "# FEAT-IT: Saved views\n\n<!-- koolade-artifact-id:v1 {{\"uid\":\"{uid}\",\"displayId\":\"FEAT-IT\",\"title\":\"Saved views\"}} -->\n\n## Intent\n\nAnalysts want reusable saved views so daily context is never lost.\n\n## Current Behavior\n\nViews cannot be saved.\n\n## Desired Behavior\n\nAnalysts can save, list, and reopen saved views.\n\n## Scope\n\n- Persist and restore named views\n- Out of scope: sharing views across users\n\n## Affected Product Areas\n\nView gallery and persistence.\n\n## Requirements\n\n- REQ-001: Saved views are restored after restart.\n\n## Decisions and Assumptions\n\n- Views are stored locally.\n\n## Acceptance Criteria\n\n1. A saved view can be reopened.\n2. The saved list survives relaunch.\n\n<!-- koolade-change:v1 {{\"schemaVersion\":1,\"uid\":\"{uid}\",\"displayId\":\"CHG-IT\",\"status\":\"draft\"}} -->\n"
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

#[test]
fn ready_plan_yields_an_in_review_card_that_survives_reload_as_a_noop() {
    let repo = repo("ready");
    let mut project = seed(&repo, &feature_uid(), true);
    assert_eq!(project.planning_work.len(), 1, "root only before sync");
    let root_uid = project.planning_work[0].uid.clone();

    let effects = sync(&mut project);
    assert!(
        effects.evaluations_wanted.is_empty(),
        "ready plans do not request evaluation turns"
    );
    assert!(
        !effects.announcements.is_empty(),
        "the new ready card is told to the user"
    );
    let card = the_card(&project);
    let card_uid = card.uid.clone();
    let payload = card.plan_confirmation.as_ref().expect("typed payload");
    let hash = payload.snapshot_content_hash.clone();
    let parent = card.parent_uid.clone();
    let ready_state = card.status;
    assert_eq!(ready_state, WorkStatus::InReview);
    assert!(payload.ready, "readiness comes from typed evidence only");
    assert!(
        payload.cause.is_none() && payload.blockers.is_empty(),
        "ready card names no blockers"
    );
    assert_eq!(
        parent,
        Some(root_uid),
        "card is owned by the TaskIntent root"
    );
    // Durability: reload from the private store and demand strict no-op.
    let mut reloaded = project_from(&repo.root);
    reloaded.planning_work = crate::core::planning_work::load(&reloaded.state.planning_store)
        .expect("work records load");
    let reloaded_card = the_card(&reloaded);
    assert_eq!(reloaded_card.uid, card_uid);
    assert_eq!(reloaded_card.status, WorkStatus::InReview);
    let reloaded_hash = reloaded_card
        .plan_confirmation
        .as_ref()
        .expect("payload survived")
        .snapshot_content_hash
        .clone();
    assert_eq!(reloaded_hash, hash);
    let before = reloaded.planning_work.clone();
    let quiet = sync(&mut reloaded);
    assert!(quiet.announcements.is_empty(), "no repeat announcements");
    assert!(quiet.evaluations_wanted.is_empty(), "no repeat evaluations");
    assert_eq!(
        reloaded.planning_work, before,
        "second pass is a strict no-op"
    );
    remove_planning_store_for(&repo.root);
}

#[test]
fn unready_plan_creates_needs_attention_and_evaluates_once_per_spec_revision() {
    let repo = repo("unready");
    let mut project = seed(&repo, &feature_uid(), false);
    let effects = sync(&mut project);
    let card = the_card(&project);
    let card_uid = card.uid.clone();
    let card_key = card.key.clone();
    let card_detail = card.detail.trim().to_owned();
    let payload = card.plan_confirmation.as_ref().expect("typed payload");
    let caused = payload.cause;
    assert!(
        !card_detail.is_empty(),
        "concrete copy explains the blockage"
    );
    assert!(!payload.ready);
    assert!(!payload.blockers.is_empty());
    assert!(matches!(
        caused,
        Some(crate::core::planning_work::PlanNotReadyCause::AssessmentNeeded)
    ));
    assert_eq!(
        effects.evaluations_wanted.len(),
        1,
        "one readiness-evaluation turn requested"
    );
    assert_eq!(
        effects.evaluations_wanted[0].work_key, card_key,
        "evaluation binds to the card"
    );
    let quiet = sync(&mut project);
    assert!(
        quiet.evaluations_wanted.is_empty(),
        "the same spec revision is never re-requested"
    );
    assert!(quiet.announcements.is_empty());
    assert_eq!(the_card(&project).uid, card_uid, "card is not rebuilt");
    remove_planning_store_for(&repo.root);
}

#[test]
fn malformed_plan_document_is_contained_without_any_evaluation_request() {
    let repo = repo("malformed");
    let mut project = seed(&repo, &feature_uid(), false);
    // Swap in a document that carries the durable markers but no reviewable
    // spec body: identity parses, the plan does not.
    let uid = project
        .planning_work
        .first()
        .expect("root")
        .feature_uid
        .clone()
        .unwrap();
    project.state.active_features = vec![(
        "FEAT-IT".to_owned(),
        format!(
            "# FEAT-IT: Broken plan\n\n<!-- koolade-artifact-id:v1 {{\"uid\":\"{uid}\",\"displayId\":\"FEAT-IT\",\"title\":\"Broken plan\"}} -->\n\nGarbage body.\n\n<!-- koolade-change:v1 {{\"schemaVersion\":1,\"uid\":\"{uid}\",\"displayId\":\"CHG-IT\",\"status\":\"draft\"}} -->\n"
        ),
    )];
    let effects = sync(&mut project);
    let card = the_card(&project);
    let card_status = card.status;
    let payload = card.plan_confirmation.as_ref().expect("typed payload");
    let caused = payload.cause;
    assert!(!card.detail.trim().is_empty());
    assert_eq!(card_status, WorkStatus::NeedsAttention);
    assert!(matches!(
        caused,
        Some(crate::core::planning_work::PlanNotReadyCause::MalformedDocument)
    ));
    assert!(
        effects.evaluations_wanted.is_empty(),
        "an unparseable plan is repaired by people, not by planner turns"
    );
    remove_planning_store_for(&repo.root);
}

#[test]
fn revised_spec_moves_the_same_card_and_keeps_demanding_a_decision() {
    let repo = repo("drift");
    let uid = feature_uid();
    let mut project = seed(&repo, &uid, true);
    sync(&mut project);
    let card_uid = the_card(&project).uid.clone();
    let first_hash = the_card(&project)
        .plan_confirmation
        .as_ref()
        .expect("payload")
        .snapshot_content_hash
        .clone();
    // Revise the spec: same durable feature identity, new requirement, hence
    // a new immutable snapshot the approver has never seen.
    let revised = spec_body(&uid) + "\n- REQ-002: The saved list shows each view’s name.\n";
    let identity = crate::domain::ArtifactIdentity::from_markdown(&revised)
        .ok()
        .flatten()
        .expect("identity parses");
    let contract = crate::core::workflow::feature_contract(&revised);
    let snapshot = plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        "FEAT-IT",
        &identity,
        &revised,
        &contract,
    )
    .expect("revised snapshot builds");
    assert_ne!(
        snapshot.content_hash, first_hash,
        "the revision must produce a distinct snapshot"
    );
    let store = &project.state.planning_store;
    let (current, current_revision) = QuestionStore::new(store)
        .load_readiness(&uid)
        .expect("readiness loads")
        .expect("prior checklist exists");
    let _ = current;
    let requirements = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.clone())
        .collect::<Vec<_>>();
    let checklist = crate::core::planning::ReadinessChecklistV1 {
        feature_uid: uid.clone(),
        plan_revision: current.plan_revision + 1,
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
        revision: current_revision,
    };
    QuestionStore::new(store)
        .save_readiness(&checklist, current_revision)
        .expect("revised readiness saves");
    project.state.active_features = vec![("FEAT-IT".to_owned(), revised)];
    let effects = sync(&mut project);
    let card = the_card(&project);
    assert_eq!(
        card.uid, card_uid,
        "revision updates, it does not recreate, the card"
    );
    assert_eq!(
        card.status,
        WorkStatus::InReview,
        "revised plan goes back in front of the decider"
    );
    let payload = card.plan_confirmation.as_ref().expect("payload");
    assert_eq!(payload.snapshot_content_hash, snapshot.content_hash);
    assert_eq!(payload.snapshot_spec_revision, snapshot.spec_revision);
    assert!(
        effects
            .announcements
            .iter()
            .any(|text| text.contains("review") || text.contains("change")),
        "the drift is surfaced, not silent"
    );
    remove_planning_store_for(&repo.root);
}

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
