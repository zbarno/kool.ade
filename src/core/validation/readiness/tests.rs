use super::*;
use crate::core::workflow::InterviewBrief;

#[test]
fn readiness_is_scoped_to_the_target_brief_and_preserves_saved_choices() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success()
    );
    let mut state = PlannerState::load(dir.path()).unwrap();
    state.active_feature = Some(("F2".into(), "# F2: Unrelated".into()));
    let envelope: crate::harness::TurnEnvelope = serde_json::from_value(serde_json::json!({
        "schema_version":2,"assistant_message":"Choice recorded", "open_items_resolved":[],"open_items_updated":[],"open_items_added":[]
    })).unwrap();
    let mut turn =
        crate::core::validation::validate(&envelope, &state, &state.effective_user()).unwrap();
    turn.change_status_updates
        .insert("F1".into(), ChangeStatus::Ready);
    turn.document_updates
        .push(("feature:F1".into(), "# F1: Example".into()));
    align_change_status(&state, &mut turn);
    assert_eq!(turn.change_status_updates["F1"], ChangeStatus::Draft);
    assert_eq!(turn.assistant_message, "Choice recorded");
    state.workflow.brief = Some(InterviewBrief {
        feature_name: "F1: Example".into(),
        ready_for_tasks: true,
        ..Default::default()
    });
    turn.change_status_updates
        .insert("F1".into(), ChangeStatus::Ready);
    align_change_status(&state, &mut turn);
    assert_eq!(
        turn.change_status_updates["F1"],
        ChangeStatus::Ready,
        "a matching target brief is independent of global feature focus"
    );
    state
        .active_features
        .push(("F2".into(), "# F2: Example".into()));
    state.workflow.brief.as_mut().unwrap().feature_name = "Example".into();
    align_change_status(&state, &mut turn);
    assert_eq!(
        turn.change_status_updates["F1"],
        ChangeStatus::Draft,
        "duplicate titles cannot lend another feature readiness"
    );
    state.workflow.brief.as_mut().unwrap().feature_name = "F1: Example".into();
    state.workflow.brief.as_mut().unwrap().ready_for_tasks = false;
    align_change_status(&state, &mut turn);
    assert_eq!(turn.change_status_updates["F1"], ChangeStatus::Draft);
    assert!(state.workflow.approved_features.is_empty());
}

#[test]
fn unrelated_brief_preserves_only_unchanged_unblocked_ready_contracts() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success()
    );
    let mut state = PlannerState::load(dir.path()).unwrap();
    let body = crate::domain::ArtifactIdentity::preserve_markdown(
        "# F1: Example\n\n**Status:** Ready\n\n## Requirements\n\nPreserve this contract.\n",
        None,
        "F1",
        "Example",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
        .unwrap()
        .unwrap();
    let body = crate::domain::ChangeMetadata::write_markdown(&body, &identity, ChangeStatus::Ready)
        .unwrap();
    state.active_features.push(("F1".into(), body.clone()));
    let envelope = serde_json::from_value(
        serde_json::json!({"schema_version":2,"assistant_message":"Saved choice"}),
    )
    .unwrap();
    let mut turn =
        crate::core::validation::validate(&envelope, &state, &state.effective_user()).unwrap();
    turn.change_status_updates
        .insert("F1".into(), ChangeStatus::Ready);
    align_change_status(&state, &mut turn);
    assert_eq!(turn.change_status_updates["F1"], ChangeStatus::Ready);
    turn.document_updates.push((
        "feature:F1".into(),
        body.replace("Preserve this contract.", "Change this contract."),
    ));
    align_change_status(&state, &mut turn);
    assert_eq!(turn.change_status_updates["F1"], ChangeStatus::Draft);
    turn.document_updates.clear();
    turn.change_status_updates
        .insert("F1".into(), ChangeStatus::Ready);
    let mut blocker = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "A new decision".into(),
        "Needs an answer".into(),
    );
    blocker.feature_id = Some("F1".into());
    turn.added.push(blocker);
    align_change_status(&state, &mut turn);
    assert_eq!(turn.change_status_updates["F1"], ChangeStatus::Draft);
}
