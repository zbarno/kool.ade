use std::fs;

use crate::{
    artifacts::{items_io, planning_store::paths},
    core::{apply, state::PlannerState, validation::NormalizedTurn},
    domain::{ItemKind, OpenItem, Priority},
};

use super::support::{TestDirectory, git_ok};

fn record_turn() -> NormalizedTurn {
    NormalizedTurn {
        assistant_message: "Feature draft saved.".into(),
        change_summary: None,
        spec_markdown: None,
        document_updates: Vec::new(),
        planning_tasks: Vec::new(),
        change_status_updates: Default::default(),
        additional_planning_artifacts: Vec::new(),
        added: Vec::new(),
        updates: Vec::new(),
        resolved: Vec::new(),
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        warnings: Vec::new(),
        workflow: None,
        task_batch: None,
        plan_comparison: None,
    }
}

#[test]
fn creates_a_clean_planning_only_workspace_with_default_documents() {
    let temp = TestDirectory::new("create");
    let manager = temp.manager("operator");

    let workspace = manager.create("Planning only").unwrap();

    assert_eq!(
        workspace.planning_repo,
        manager.planning_repo_path(workspace.id)
    );
    assert!(manager.association_path(workspace.id).is_file());
    assert!(workspace.association.code_checkouts.is_empty());
    assert!(workspace.repositories.repositories.is_empty());
    assert!(
        workspace
            .planning_repo
            .join("config/repositories.json")
            .is_file()
    );
    assert!(workspace.planning_repo.join("config/project.md").is_file());
    assert!(
        workspace
            .planning_repo
            .join("planning/product/index.md")
            .is_file()
    );
    assert!(
        workspace
            .planning_repo
            .join("planning/product/manifest.json")
            .is_file()
    );
    assert!(
        fs::read_to_string(workspace.planning_repo.join("planning/product/index.md"))
            .unwrap()
            .contains("Planning only — Living Technical Specification")
    );
    for legacy in [
        "planning/open-items.md",
        "planning/resolved-items.json",
        "state/work.json",
        "state/workflow.json",
    ] {
        assert!(!workspace.planning_repo.join(legacy).exists(), "{legacy}");
    }
    assert!(
        git_ok(
            Some(&workspace.planning_repo),
            &["branch", "--show-current"]
        )
        .stdout
        .starts_with(b"main\n")
    );
    assert_eq!(
        git_ok(Some(&workspace.planning_repo), &["status", "--porcelain"]).stdout,
        b""
    );
    assert!(manager.association_path(workspace.id).is_file());
    assert_eq!(workspace.id, workspace.manifest.workspace_id);
}

#[test]
fn planning_only_workspace_can_save_a_feature_and_question_as_normalized_records() {
    let temp = TestDirectory::new("planning-only-board");
    let manager = temp.manager("operator");
    let workspace = manager.create("Questions welcome").unwrap();
    let mut state =
        PlannerState::load_with_store(&workspace.planning_repo, &workspace.store).unwrap();
    state.bootstrap_missing().unwrap();

    let feature_id = crate::artifacts::product_docs::next_feature_id(&workspace.store);
    let feature = format!(
        "# {feature_id}: Saved searches\n\n**Status:** Draft\n\n## Intent\n\nSave named searches.\n\n## Current Behavior\n\nSearches are temporary.\n\n## Desired Behavior\n\nUsers can save searches.\n\n## Scope\n\nSearch controls only.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nSaved searches reopen.\n\n## Decisions and Assumptions\n\nNone yet.\n\n## Acceptance Criteria\n\nSaved searches reopen after restart.\n"
    );
    let mut question = OpenItem::new(
        "CLR-001".into(),
        Priority::Normal,
        ItemKind::Question,
        "Product".into(),
        None,
        "Should saved searches be shared?".into(),
        "The feature draft leaves sharing open.".into(),
    );
    question.feature_id = Some(feature_id.clone());
    let mut turn = record_turn();
    turn.document_updates
        .push((format!("feature:{feature_id}"), feature));
    turn.change_status_updates
        .insert(feature_id.clone(), crate::domain::ChangeStatus::Draft);
    turn.added.push(question);

    apply::apply(&mut state, &turn).unwrap();

    let feature_path = crate::artifacts::product_docs::document_path(
        &workspace.store,
        &format!("feature:{feature_id}"),
    )
    .unwrap();
    assert!(feature_path.is_file());
    let (items, resolved, _) = items_io::load_store(&workspace.store).unwrap();
    assert!(resolved.is_empty());
    let question = items
        .iter()
        .find(|item| item.id == "CLR-001")
        .expect("the explicit planning question is saved");
    assert!(question.feature_uid.is_some());
    assert!(workspace.planning_repo.join(paths::ITEM_CONTENTS).is_dir());
    assert!(
        !workspace
            .planning_repo
            .join("planning/open-items.md")
            .exists()
    );
    assert!(!workspace.planning_repo.join(".koolade-packet").exists());

    assert!(feature_path.starts_with(workspace.planning_repo.join("planning/changes")));
    assert!(
        fs::read_to_string(feature_path)
            .unwrap()
            .contains("Saved searches")
    );
}

#[test]
fn restart_reuses_workspace_identity_head_and_documents() {
    let temp = TestDirectory::new("restart");
    let manager = temp.manager("operator");
    let created = manager.create("Restartable").unwrap();
    let head = git_ok(Some(&created.planning_repo), &["rev-parse", "HEAD"]).stdout;
    let project_doc = fs::read(created.planning_repo.join("config/project.md")).unwrap();

    let opened = manager.open(created.id).unwrap();

    assert_eq!(opened.id, created.id);
    assert_eq!(
        git_ok(Some(&opened.planning_repo), &["rev-parse", "HEAD"]).stdout,
        head
    );
    assert_eq!(
        fs::read(opened.planning_repo.join("config/project.md")).unwrap(),
        project_doc
    );
}
