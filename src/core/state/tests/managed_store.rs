use super::*;
#[test]
fn separate_installations_share_published_specs_but_not_board_or_conversations() {
    let code = git_fixture("installations-code", Some("Koolade Test"));
    crate::core::state::test_support::seed_published_product_document(&code).unwrap();
    let local_first = mkrepo_without_git("installation-first-local");
    let local_second = mkrepo_without_git("installation-second-local");
    let project_id = test_support::project_id_for_code_root(&code);
    let first_store = PlanningStore::for_code_repository(project_id, &local_first, &code);
    let second_store = PlanningStore::for_code_repository(project_id, &local_second, &code);

    let first_state = PlannerState::load_with_store(&code, &first_store).unwrap();
    let second_state = PlannerState::load_with_store(&code, &second_store).unwrap();
    assert_eq!(
        first_state.published_spec_pin,
        second_state.published_spec_pin
    );
    assert_eq!(first_state.spec_text, second_state.spec_text);
    assert_ne!(
        first_state.planning_store.root,
        second_state.planning_store.root
    );

    let work = crate::core::planning_work::Work::new(
        "planning:private-one".into(),
        "Private board item".into(),
        "Only installation one owns this work.".into(),
        "Waiting".into(),
    );
    crate::core::planning_work::save(&first_store, std::slice::from_ref(&work)).unwrap();
    assert_eq!(
        crate::core::planning_work::load(&first_store)
            .unwrap()
            .len(),
        1
    );
    assert!(
        crate::core::planning_work::load(&second_store)
            .unwrap()
            .is_empty()
    );

    let first_home = mkrepo_without_git("installation-first-home");
    let slug = project_id.hyphenated().to_string();
    {
        let _home = crate::test_support::KooladeHomeGuard::set(&first_home);
        let mut chats = crate::persistence::task_chats::TaskChats::default();
        chats.ensure_loaded(&slug);
        chats
            .append(
                &slug,
                "task-one",
                vec![crate::domain::ChatMessage::new(
                    crate::domain::ChatRole::User,
                    "Private conversation one.",
                    None,
                )],
            )
            .unwrap();
    }
    let second_home = mkrepo_without_git("installation-second-home");
    {
        let _home = crate::test_support::KooladeHomeGuard::set(&second_home);
        let mut chats = crate::persistence::task_chats::TaskChats::default();
        chats.ensure_loaded(&slug);
        assert!(chats.messages.is_empty());
    }
    assert!(!local_first.join(".git").exists());
    assert!(!local_second.join(".git").exists());

    for path in [code, local_first, local_second, first_home, second_home] {
        let _ = std::fs::remove_dir_all(path);
    }
}

#[test]
fn pinned_code_product_documents_remain_at_loaded_revision_across_checkout_updates() {
    let code = git_fixture("pinned-product-code", Some("Koolade Test"));
    let overview = code.join("docs/koolade/product/overview.md");
    std::fs::create_dir_all(overview.parent().unwrap()).unwrap();
    std::fs::write(
        &overview,
        "# Pinned Product — Living Technical Specification\n\n## Overview\n\nPublished revision A.\n",
    )
    .unwrap();
    git(&code, &["add", "docs/koolade/product/overview.md"]);
    git(&code, &["commit", "-q", "-m", "published product A"]);

    let local = mkrepo_without_git("pinned-product-local");
    let mut store = PlanningStore::for_code_repository(uuid::Uuid::new_v4(), &local, &code);

    let pinned = crate::core::published_specs::PublishedSpecReader::for_primary_repository(&code)
        .unwrap()
        .pin()
        .unwrap();
    store.published_spec_pin = Some(pinned.clone());
    let state_a = PlannerState::load_with_store(&code, &store).unwrap();
    assert!(
        state_a
            .spec_text
            .as_deref()
            .unwrap()
            .contains("Published revision A")
    );
    assert!(!local.join("planning/product/overview.md").exists());
    assert!(
        crate::artifacts::product_docs::render_product(&store)
            .unwrap()
            .unwrap()
            .contains("Published revision A")
    );

    std::fs::write(
        &overview,
        "# Pinned Product — Living Technical Specification\n\n## Overview\n\nPublished revision B.\n",
    )
    .unwrap();
    git(&code, &["add", "docs/koolade/product/overview.md"]);
    git(&code, &["commit", "-q", "-m", "published product B"]);
    let state_still_a = PlannerState::load_with_store(&code, &store).unwrap();
    assert!(
        state_still_a
            .spec_text
            .as_deref()
            .unwrap()
            .contains("Published revision A")
    );
    let mut refreshed = store.clone();
    refreshed.published_spec_pin = None;
    let state_b = PlannerState::load_with_store(&code, &refreshed).unwrap();
    assert!(
        state_b
            .spec_text
            .as_deref()
            .unwrap()
            .contains("Published revision B")
    );
    assert!(
        !state_b
            .spec_text
            .as_deref()
            .unwrap()
            .contains("Stale local content")
    );

    let _ = std::fs::remove_dir_all(code);
    let _ = std::fs::remove_dir_all(local);
}

#[test]
fn private_task_state_and_spec_drafts_survive_code_branch_switch() {
    use crate::core::validation::NormalizedTurn;
    use crate::domain::{ChangeStatus, ItemKind, OpenItem, Priority};

    let code = mkrepo("managed-lifecycle-code");
    git(&code, &["branch", "-M", "main"]);
    git(&code, &["config", "user.name", "Koolade Test"]);
    git(&code, &["config", "user.email", "koolade@example.test"]);
    std::fs::write(code.join("README.md"), "code checkout\n").unwrap();
    git(&code, &["add", "README.md"]);
    git(&code, &["commit", "-q", "-m", "fixture"]);
    crate::core::state::test_support::seed_published_product_document(&code).unwrap();
    git(&code, &["checkout", "-q", "-b", "feature/auth"]);

    let local_root = mkrepo_without_git("managed-lifecycle-local");
    let store = PlanningStore::for_code_repository(uuid::Uuid::new_v4(), &local_root, &code);
    let mut state = PlannerState::load_with_store(&code, &store).unwrap();
    state.bootstrap_missing().unwrap();

    let feature = "# F1: Shared import context\n\n## Intent\n\nExpose stored reference content to planning turns.\n\n## Current Behavior\n\nImports are listed by path.\n\n## Desired Behavior\n\nManaged imports are available to the planner.\n\n## Scope\n\nPlanning context only.\n\n## Affected Product Areas\n\n`product:overview`\n\n## Requirements\n\n- FR-1 (MUST). Read managed imports from the selected local store.\n\n## Decisions and Assumptions\n\n- **D-1:** Imported documents are reference data.\n\n## Acceptance Criteria\n\n1. The planner can use a managed imported document.\n";
    let mut item = OpenItem::new(
        "CLR-001".into(),
        Priority::Normal,
        ItemKind::Question,
        "Product".into(),
        "Should imported text be inlined?".into(),
        "Workers do not read private task state from the source repository.".into(),
    );
    item.feature_id = Some("F1".into());
    let turn = NormalizedTurn {
        assistant_message: "Created a private feature proposal.".into(),
        change_summary: Some("Create private feature proposal".into()),
        document_updates: vec![
            (
                "product:overview".into(),
                "# Overview\n\nManaged planning data remains stable across code branches.\n".into(),
            ),
            ("feature:F1".into(), feature.into()),
        ],
        planning_tasks: Vec::new(),
        task_intent_context: None,
        change_status_updates: [("F1".into(), ChangeStatus::Draft)].into(),
        additional_planning_artifacts: Vec::new(),
        added: vec![item],
        updates: Vec::new(),
        resolved: Vec::new(),
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        warnings: Vec::new(),
        workflow: None,
        task_batch: None,
        plan_comparison: None,
    };
    let receipt = crate::core::apply::apply(&mut state, &turn).unwrap();
    assert!(receipt.draft_written && receipt.items_written);
    let proposal = crate::core::apply::load_current(&store).unwrap().unwrap();
    assert!(
        proposal
            .changes
            .iter()
            .any(|change| change.document_id == "feature:F1")
    );
    assert!(store.root.join("state/spec_drafts/current.json").is_file());
    assert!(!local_root.join(".git").exists());
    assert!(!code.join(".koolade-packet").exists());
    let before_switch = snapshot(&local_root);

    git(&code, &["checkout", "-q", "main"]);
    let reopened = PlannerState::load_with_store(&code, &store).unwrap();
    assert_eq!(reopened.planning_store.root, store.root);
    assert_eq!(
        reopened
            .items
            .iter()
            .find(|item| item.id == "CLR-001")
            .and_then(|item| item.feature_id.as_deref()),
        Some("F1")
    );
    assert!(reopened.local_spec_draft.is_some());
    assert_eq!(snapshot(&local_root), before_switch);
    assert!(!code.join("planning").exists());

    let _ = std::fs::remove_dir_all(code);
    let _ = std::fs::remove_dir_all(local_root);
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn snapshot(root: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn visit(
        root: &std::path::Path,
        current: &std::path::Path,
        files: &mut std::collections::BTreeMap<String, Vec<u8>>,
    ) {
        for entry in std::fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, files);
            } else if entry.file_type().unwrap().is_file() {
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    visit(root, root, &mut files);
    files
}
