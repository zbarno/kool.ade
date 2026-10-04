use super::*;

#[test]
fn happy_path_writes_files_and_commits() {
    let (inputs, dir) = inputs_for("happy", "please draft the initial spec");
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
            assigned_to: Some("All".into()),
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
            commit_result,
            ..
        } => {
            assert_eq!(receipt.repo_relative_paths.len(), 3);
            assert!(receipt.commit_message.starts_with("planner: "));
            assert!(commit_result.is_ok(), "commit failed: {commit_result:?}");
            assert_eq!(state.items.len(), 1);
        }
        other => panic!("expected Applied, got: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modular_turn_changes_only_named_modules_and_rejects_bad_id_atomically() {
    let (mut inputs, dir) = inputs_for("modular_turn", "refine product scope");
    git_stdout(&dir, &["add", "-A"]);
    git_stdout(&dir, &["commit", "-m", "Seed modular product"]);
    inputs.state = PlannerState::load(&dir).unwrap();
    let product = dir.join(".koolade-packet/planning/product");
    let vision = product.join("overview.md");
    let scope = product.join("users-and-outcomes.md");
    let unrelated = product.join("current-capabilities.md");
    let original_unrelated = std::fs::read(&unrelated).unwrap();
    let changed_vision = "# Overview\n\nCurrent purpose from inspected evidence.\n";
    let changed_scope = "# Users and Outcomes\n\nCurrent scope from accepted intent.\n";
    let env = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Updated two product areas.".into()),
        change_summary: Some("Refine product vision and scope".into()),
        document_updates: Some(vec![
            crate::harness::DocumentUpdate {
                document_id: "product:overview".into(),
                content: changed_vision.into(),
                status: None,
            },
            crate::harness::DocumentUpdate {
                document_id: "product:users-and-outcomes".into(),
                content: changed_scope.into(),
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
    };
    let result = drain(&TurnController::start(
        inputs.clone(),
        Box::new(ScriptedHarness {
            canned: Some(env),
            raw: None,
        }),
    ));
    match result {
        TurnOutcome::Applied {
            receipt,
            commit_result,
            ..
        } => {
            assert!(commit_result.is_ok());
            assert_eq!(receipt.repo_relative_paths.len(), 3);
        }
        other => panic!("expected modular apply, got {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&vision).unwrap(), changed_vision);
    assert_eq!(std::fs::read_to_string(&scope).unwrap(), changed_scope);
    assert_eq!(std::fs::read(&unrelated).unwrap(), original_unrelated);
    let before_commit = git_stdout(&dir, &["rev-list", "--count", "HEAD"]);
    let before_vision = std::fs::read(&vision).unwrap();
    inputs.state = PlannerState::load(&dir).unwrap();
    let bad = serde_json::json!({"schema_version":2,"assistant_message":"Changed scope",
            "document_updates":[{"document_id":"product:overview","content":"# Overview\n\nWrong\n"},
                {"document_id":"product:../../escape","content":"bad"}],
            "open_items_added":[{"kind":"Question","priority":"Normal","authority":"Human",
                "category":"General","assigned_to":"All","question":"Should this ship?","reason":"Release decision"}]}).to_string();
    let result = drain(&TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: None,
            raw: Some(bad),
        }),
    ));
    assert!(matches!(result, TurnOutcome::Rejected { .. }));
    assert_eq!(std::fs::read(&vision).unwrap(), before_vision);
    assert_eq!(std::fs::read(&unrelated).unwrap(), original_unrelated);
    assert_eq!(
        git_stdout(&dir, &["rev-list", "--count", "HEAD"]),
        before_commit
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn planner_can_add_a_project_specific_module_and_apply_it_atomically() {
    let (mut inputs, dir) = inputs_for(
        "adaptive_module",
        "record this project's billing integration",
    );
    git_stdout(&dir, &["add", "-A"]);
    git_stdout(&dir, &["commit", "-m", "Seed adaptive product"]);
    inputs.state = PlannerState::load(&dir).unwrap();
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
        commit_result,
        ..
    } = result
    else {
        panic!("expected adaptive module apply, got {result:?}");
    };
    assert!(commit_result.is_ok());
    let layout = crate::artifacts::layout::ArtifactLayout::new(&dir);
    let manifest = crate::artifacts::product_docs::load_manifest(&dir)
        .unwrap()
        .unwrap();
    assert_eq!(manifest.modules.last().unwrap().id, "billing");
    assert_eq!(
        std::fs::read_to_string(layout.product_root().join("billing.md")).unwrap(),
        content
    );
    assert!(
        std::fs::read_to_string(layout.product_index())
            .unwrap()
            .contains("- [Billing](billing.md)")
    );
    assert!(
        receipt
            .repo_relative_paths
            .contains(&crate::artifacts::layout::canonical::PRODUCT_MANIFEST.to_owned())
    );
    assert!(
        crate::artifacts::product_docs::render_product(&dir)
            .unwrap()
            .unwrap()
            .contains("## Billing\n\nThe application uses the Acme billing service")
    );
}

#[test]
fn modular_turn_creates_next_feature_and_indexes_it() {
    let (mut inputs, dir) = inputs_for("new_feature", "Plan saved searches");
    git_stdout(&dir, &["add", "-A"]);
    git_stdout(&dir, &["commit", "-m", "Seed modular product"]);
    inputs.state = PlannerState::load(&dir).unwrap();
    let feature = "# F1: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave repeated searches.\n\n## Current Behavior\n\nNo saved searches observed.\n\n## Desired Behavior\n\nUsers can save searches.\n\n## Scope\n\nSearch UI only.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nSave and restore.\n\n## Decisions and Assumptions\n\nNone yet.\n\n## Acceptance Criteria\n\nA saved search reopens.\n";
    let env = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Drafted saved searches.".into()),
        change_summary: Some("Draft saved searches".into()),
        document_updates: Some(vec![crate::harness::DocumentUpdate {
            document_id: "feature:F1".into(),
            content: feature.into(),
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
    };
    let result = drain(&TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: Some(env),
            raw: None,
        }),
    ));
    match result {
        TurnOutcome::Applied {
            receipt,
            commit_result,
            state,
            ..
        } => {
            assert!(commit_result.is_ok());
            assert_eq!(receipt.repo_relative_paths.len(), 2);
            let contract = crate::core::contract_snapshot::freeze(&state)
                .unwrap()
                .unwrap();
            assert_eq!(contract.feature_id, "F1");
            assert!(
                contract
                    .product_modules
                    .contains_key("current-capabilities")
            );
            assert!(contract.repository_bases.contains_key("root"));
        }
        other => panic!("expected new feature, got {other:?}"),
    }
    let saved_feature = std::fs::read_to_string(
        dir.join(".koolade-packet/planning/changes/F1-saved-searches/specification.md"),
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&saved_feature)
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
        std::fs::read_to_string(dir.join(".koolade-packet/planning/product/index.md"))
            .unwrap()
            .contains("F1-saved-searches")
    );
    assert_eq!(crate::artifacts::product_docs::next_feature_id(&dir), "F2");
    let _ = std::fs::remove_dir_all(dir);
}
