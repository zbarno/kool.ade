use super::*;

fn draft_changes(
    store: &crate::artifacts::planning_store::PlanningStore,
    changed_paths: &[String],
) -> Vec<(String, String)> {
    assert!(
        changed_paths
            .iter()
            .any(|path| path == "state/spec_drafts/current.json")
    );
    crate::core::apply::load_current(store)
        .unwrap()
        .expect("document updates are saved as a private specification draft")
        .changes
        .into_iter()
        .map(|change| (change.document_id, change.content))
        .collect()
}

fn private_draft_files(
    store: &crate::artifacts::planning_store::PlanningStore,
) -> Vec<(String, Vec<u8>)> {
    let root = store.root.join("state/spec_drafts");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut files = entries
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(path).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

#[test]
fn happy_path_writes_private_state_without_committing_code() {
    let (inputs, dir) = inputs_for("happy", "please draft the initial spec");
    let code_head = git_stdout(&dir, &["rev-parse", "HEAD"]);
    let store = inputs.state.planning_store.clone();
    let env = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Drafted an initial spec and raised the first question.".into()),
        change_summary: Some("Draft initial specification".into()),
        document_updates: Some(vision_update("Demo the planner end-to-end.")),
        planning_tasks: None,
        updated_specification: None,
        open_items_added: Some(vec![TurnItem {
            authority: None,
            id: None,
            kind: Some("Question".into()),
            category: Some("General".into()),
            priority: Some("Normal".into()),
            question: Some("Which deployment target first?".into()),
            reason: Some("packaging depends on it".into()),
            resolution_note: None,
            feature_id: None,
            recommendation: None,
            evidence: None,
            decision_brief: None,
            blocked_by: Vec::new(),
        }]),
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: None,
        interview: None,
        task_stories: None,
        requested_action: None,
        follow_up_task: None,
        task_outline: None,
        plans: None,
        recommendation: None,
        plan_remediation: None,
    };
    let c = TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: Some(env),
            raw: None,
        }),
    );
    match drain(&c) {
        TurnOutcome::Applied {
            state,
            receipt,
            save_result,
            ..
        } => {
            assert_eq!(receipt.changed_store_paths.len(), 4);
            assert_eq!(receipt.summary, "draft initial specification");
            assert!(save_result.is_ok(), "private save failed: {save_result:?}");
            assert_eq!(state.items.len(), 1);
            let changes = draft_changes(&store, &receipt.changed_store_paths);
            assert!(changes.iter().any(|(id, content)| {
                id == "product:overview" && content.contains("Demo the planner end-to-end.")
            }));
        }
        other => panic!("expected Applied, got: {other:?}"),
    }
    assert_eq!(git_stdout(&dir, &["rev-parse", "HEAD"]), code_head);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modular_turn_changes_only_named_modules_and_rejects_bad_id_atomically() {
    let (mut inputs, dir) = inputs_for("modular_turn", "refine product scope");
    let store = inputs.state.planning_store.clone();
    let published_overview = dir.join("docs/koolade/product/overview.md");
    let published_scope = dir.join("docs/koolade/product/users-and-outcomes.md");
    let original_overview = std::fs::read(&published_overview).unwrap();
    let original_scope = std::fs::read(&published_scope).unwrap();
    let code_head = git_stdout(&dir, &["rev-parse", "HEAD"]);
    let changed_vision = "# Overview\n\nCurrent purpose from inspected evidence.\n";
    let changed_scope = "# Users and Outcomes\n\nCurrent scope from accepted intent.\n";
    let env = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Updated two product areas.".into()),
        change_summary: Some("Refine product vision and scope".into()),
        document_updates: Some(vec![
            crate::harness::DocumentUpdate {
                document_id: "product:overview".into(),
                content: changed_vision.to_owned(),
                status: None,
            },
            crate::harness::DocumentUpdate {
                document_id: "product:users-and-outcomes".into(),
                content: changed_scope.to_owned(),
                status: None,
            },
        ]),
        planning_tasks: None,
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: None,
        interview: None,
        task_stories: None,
        requested_action: None,
        follow_up_task: None,
        task_outline: None,
        plans: None,
        recommendation: None,
        plan_remediation: None,
    };
    let result = drain(&TurnController::start(
        inputs.clone(),
        Box::new(ScriptedHarness {
            canned: Some(env),
            raw: None,
        }),
    ));
    let changed_paths = match result {
        TurnOutcome::Applied {
            receipt,
            save_result,
            ..
        } => {
            assert!(save_result.is_ok());
            assert_eq!(receipt.changed_store_paths.len(), 2);
            let changes = draft_changes(&store, &receipt.changed_store_paths);
            assert_eq!(changes.len(), 2);
            assert!(changes.contains(&("product:overview".into(), changed_vision.into())));
            assert!(changes.contains(&("product:users-and-outcomes".into(), changed_scope.into())));
            receipt.changed_store_paths
        }
        other => panic!("expected modular apply, got {other:?}"),
    };
    assert_eq!(
        std::fs::read(&published_overview).unwrap(),
        original_overview
    );
    assert_eq!(std::fs::read(&published_scope).unwrap(), original_scope);
    assert_eq!(git_stdout(&dir, &["rev-parse", "HEAD"]), code_head);
    let before_commit = git_stdout(&dir, &["rev-list", "--count", "HEAD"]);
    let before_drafts = private_draft_files(&store);
    inputs.state = PlannerState::load_with_store(&dir, &store).unwrap();
    let bad = serde_json::json!({"schema_version":2,"assistant_message":"Changed scope",
            "document_updates":[{"document_id":"product:overview","content":"# Overview\n\nWrong\n"},
                {"document_id":"product:../../escape","content":"bad"}],
            "open_items_added":[{"kind":"Question","priority":"Normal","authority":"Human",
                "category":"General","question":"Should this ship?","reason":"Release decision"}]}).to_string();
    let result = drain(&TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: None,
            raw: Some(bad),
        }),
    ));
    assert!(matches!(result, TurnOutcome::Rejected { .. }));
    assert_eq!(private_draft_files(&store), before_drafts);
    assert_eq!(changed_paths.len(), 2);
    assert_eq!(
        git_stdout(&dir, &["rev-list", "--count", "HEAD"]),
        before_commit
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn planner_can_add_a_project_specific_module_and_apply_it_atomically() {
    let (inputs, dir) = inputs_for(
        "adaptive_module",
        "record this project's billing integration",
    );
    let store = inputs.state.planning_store.clone();
    let code_head = git_stdout(&dir, &["rev-parse", "HEAD"]);
    let content = "# Billing\n\nThe application uses the Acme billing service for subscriptions.\n";
    let env = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Recorded the billing integration.".into()),
        change_summary: Some("record billing integration".into()),
        document_updates: Some(vec![crate::harness::DocumentUpdate {
            document_id: "product:billing".into(),
            content: content.into(),
            status: None,
        }]),
        planning_tasks: None,
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: None,
        interview: None,
        task_stories: None,
        requested_action: None,
        follow_up_task: None,
        task_outline: None,
        plans: None,
        recommendation: None,
        plan_remediation: None,
    };
    let result = drain(&TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: Some(env),
            raw: None,
        }),
    ));
    let TurnOutcome::Applied {
        receipt,
        save_result,
        ..
    } = result
    else {
        panic!("expected adaptive module apply, got {result:?}");
    };
    assert!(save_result.is_ok());
    assert_eq!(receipt.changed_store_paths.len(), 2);
    assert_eq!(
        draft_changes(&store, &receipt.changed_store_paths),
        vec![("product:billing".into(), content.into())]
    );
    let manifest = crate::artifacts::product_docs::load_manifest(&store)
        .unwrap()
        .unwrap();
    assert!(!manifest.modules.iter().any(|module| module.id == "billing"));
    assert_eq!(git_stdout(&dir, &["rev-parse", "HEAD"]), code_head);
}

#[test]
fn modular_turn_keeps_new_feature_as_a_private_draft_until_publication() {
    let (inputs, dir) = inputs_for("new_feature", "Plan saved searches");
    let store = inputs.state.planning_store.clone();
    let feature = "# F1: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave repeated searches.\n\n## Current Behavior\n\nNo saved searches observed.\n\n## Desired Behavior\n\nUsers can save searches.\n\n## Scope\n\nSearch UI only.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nSave and restore.\n\n## Decisions and Assumptions\n\nNone yet.\n\n## Acceptance Criteria\n\nA saved search reopens.\n";
    let env = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Drafted saved searches.".into()),
        change_summary: Some("Draft saved searches".into()),
        document_updates: Some(vec![crate::harness::DocumentUpdate {
            document_id: "feature:F1".into(),
            content: feature.to_owned(),
            status: Some(crate::domain::ChangeStatus::Draft),
        }]),
        planning_tasks: None,
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: None,
        interview: None,
        task_stories: None,
        requested_action: None,
        follow_up_task: None,
        task_outline: None,
        plans: None,
        recommendation: None,
        plan_remediation: None,
    };
    let result = drain(&TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: Some(env),
            raw: None,
        }),
    ));
    let (draft_paths, state) = match result {
        TurnOutcome::Applied {
            receipt,
            save_result,
            state,
            ..
        } => {
            assert!(save_result.is_ok());
            assert_eq!(receipt.changed_store_paths.len(), 2);
            (receipt.changed_store_paths, state)
        }
        other => panic!("expected new feature, got {other:?}"),
    };
    let changes = draft_changes(&store, &draft_paths);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0, "feature:F1");
    let saved_feature = &changes[0].1;
    let identity = crate::domain::ArtifactIdentity::from_markdown(saved_feature)
        .unwrap()
        .unwrap();
    assert_eq!(identity.display_id, "F1");
    assert_eq!(identity.title, "Saved searches");
    let visible_feature = saved_feature
        .lines()
        .filter(|line| {
            !line.starts_with("<!-- koolade-artifact-id:v1 ")
                && !line.starts_with("<!-- koolade-change:v1 ")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert_eq!(visible_feature, feature);
    assert!(
        state
            .active_features
            .iter()
            .any(|(id, body)| { id == "F1" && body.contains("**Status:** Draft") })
    );
    assert!(
        !std::fs::read_to_string(dir.join("docs/koolade/features/F1.md"))
            .unwrap_or_default()
            .contains("Saved searches")
    );
    assert!(
        !store
            .root
            .join("planning/changes/F1-saved-searches")
            .exists()
    );
    let _ = std::fs::remove_dir_all(dir);
}
