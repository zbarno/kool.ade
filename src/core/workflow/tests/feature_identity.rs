use super::*;
use crate::artifacts::planning_store::PlanningStore;

fn feature_document(state_tag: &str, root: &std::path::Path, id: &str, title: &str) -> String {
    let dir = root
        .join(".koolade-packet/planning/changes")
        .join(format!("{id}-fixture-{state_tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    let body = format!(
        "# {id}: {title}\n\n**Status:** Ready\n\n## Intent\n\nFixture intent.\n\n## Current Behavior\n\nFixture current.\n\n## Desired Behavior\n\nFixture desired.\n\n## Scope\n\nIn: fixture.\n\n## Affected Product Areas\n\nOverview.\n\n## Requirements\n\n- FIXTURE-R1 (MUST). fixture behavior.\n\n## Decisions and Assumptions\n\n- **A1 (fixture):** recorded.\n\n## Acceptance Criteria\n\n1. Observable fixture outcome.\n"
    );
    let path = dir.join("specification.md");
    let body =
        crate::artifacts::product_docs::identity::preserve_feature_identity(&path, id, &body)
            .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
        .unwrap()
        .unwrap();
    let body = crate::domain::ChangeMetadata::write_markdown(
        &body,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    std::fs::write(path, &body).unwrap();
    body
}

#[test]
fn legacy_store_routes_spec_edit_approval_and_task_generation_to_embedded_paths() {
    let project = state("store-compatibility");
    let code_root = project.repo_root.clone();
    let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &code_root);
    assert_eq!(store.root, code_root.join(".koolade-packet"));

    let spec = "# Overview\n\nUpdated through the injected planning store.\n";
    let updates = vec![("product:overview".to_owned(), spec.to_owned())];
    let layout = store.layout();
    let module_path =
        crate::artifacts::product_docs::document_path_for_update(&store, &updates[0].0, spec)
            .unwrap();
    let manifest = crate::artifacts::product_docs::updated_manifest(&store, &updates).unwrap();
    let index = crate::artifacts::product_docs::refreshed_index(&store, &updates).unwrap();
    let module_relative = module_path
        .strip_prefix(layout.root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let changes = vec![
        (module_relative, spec.as_bytes().to_vec()),
        (
            crate::artifacts::planning_store::paths::PRODUCT_MANIFEST.to_owned(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        ),
        (
            crate::artifacts::planning_store::paths::PRODUCT_INDEX.to_owned(),
            index.into_bytes(),
        ),
    ];
    let revision = store.revision().unwrap();
    store.transaction(&changes, Some(&revision)).unwrap();
    assert_eq!(std::fs::read(module_path).unwrap(), spec.as_bytes());

    let feature = feature_document(
        "store-compatibility",
        &code_root,
        "CHG-097",
        "Saved searches",
    );
    for (key, value) in [
        ("user.name", "Koolade Test"),
        ("user.email", "koolade@example.test"),
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&code_root)
                .args(["config", key, value])
                .status()
                .unwrap()
                .success()
        );
    }
    let mut workflow = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    crate::core::workflow::approve_feature(&store, &mut workflow, "CHG-097").unwrap();
    assert!(crate::core::workflow::feature_approved(
        &store, &workflow, "CHG-097"
    ));

    let mut task_brief = brief();
    task_brief.feature_name = "Saved searches (CHG-097)".into();
    let batch = TaskBatch {
        brief: task_brief,
        specification: feature.clone(),
        feature_id: Some("CHG-097".into()),
        contract: None,
        branch_targets: None,
        task_routing: TaskRoutingSnapshot::default(),
        stories: vec![story()],
    };
    let generated =
        crate::artifacts::task_docs::write_batch(&store, &batch, &mut workflow).unwrap();

    assert!(
        generated
            .iter()
            .any(|path| path.starts_with(".koolade-packet/planning/tasks/"))
    );
    assert!(
        generated
            .iter()
            .any(|path| path.starts_with(".koolade-packet/state/workflow/"))
    );
    assert_eq!(
        std::fs::read(layout.product_root().join("overview.md")).unwrap(),
        spec.as_bytes()
    );
    assert!(!store.layout().workflow_state().exists());
    assert!(
        !store
            .list_files(crate::artifacts::planning_store::paths::WORKFLOW_RECORDS)
            .unwrap()
            .is_empty()
    );
    let board = crate::artifacts::task_docs::load_board(&store, &workflow);
    let task_documents = board
        .iter()
        .filter(|document| document.path.ends_with(".md") && document.metadata.is_some())
        .collect::<Vec<_>>();
    assert!(!task_documents.is_empty());
    assert!(task_documents.iter().all(|document| {
        document
            .task_state
            .as_ref()
            .is_some_and(|state| state.status == crate::core::planning_work::WorkStatus::Todo)
    }));
    assert_eq!(
        store
            .list_files(crate::artifacts::planning_store::paths::TASK_STATES)
            .unwrap()
            .len(),
        task_documents.len()
    );
    assert!(!code_root.join("planning/tasks").exists());

    let _ = std::fs::remove_dir_all(project.repo_root);
}

#[test]
fn brief_targeting_an_inactive_feature_cannot_generate_that_batch() {
    let mut s = state("gen-target-mismatch");
    let _inactive = feature_document(
        "gen-target-mismatch",
        &s.repo_root,
        "CHG-098",
        "Other feature",
    );
    let active = feature_document(
        "gen-target-mismatch",
        &s.repo_root,
        "CHG-097",
        "Active feature",
    );
    s.active_feature = Some(("CHG-097".into(), active.clone()));
    s.workflow
        .approved_features
        .insert("CHG-097".into(), feature_contract(&active));
    let mut b = brief();
    b.feature_name = "Other feature (CHG-098)".into();
    s.workflow.brief = Some(b);
    s.workflow.reviewed_specification = Some(active);
    let joined = generation(&s, vec![story()]).unwrap_err().join(" | ");
    assert!(
        joined.contains("targets CHG-098") && joined.contains("active feature is CHG-097"),
        "guard must name both sides of the drift: {joined}"
    );
    assert!(!s.repo_root.join(".koolade-packet/planning/tasks").exists());
    std::fs::remove_dir_all(s.repo_root).unwrap();
}

#[test]
fn brief_matching_the_active_feature_passes_the_identity_guard() {
    let mut s = state("gen-target-match");
    let active = feature_document(
        "gen-target-match",
        &s.repo_root,
        "CHG-097",
        "Active feature",
    );
    s.active_feature = Some(("CHG-097".into(), active.clone()));
    s.workflow
        .approved_features
        .insert("CHG-097".into(), feature_contract(&active));
    let mut b = brief();
    b.feature_name = "Active feature (CHG-097)".into();
    s.workflow.brief = Some(b);
    s.workflow.reviewed_specification = Some(active);
    if let Err(errors) = generation(&s, vec![story()]) {
        let joined = errors.join(" | ");
        assert!(
            !joined.contains("Brief ") && !joined.contains("feature id"),
            "identity guard must not trip on a matching target: {joined}"
        );
    }
    std::fs::remove_dir_all(s.repo_root).unwrap();
}

#[test]
fn feature_id_scan_handles_legacy_prose_duplicates_and_short_ids() {
    assert_eq!(
        feature_ids_in("Kool.ad/e MVP \u{2014} git-native desktop specification planner"),
        Vec::<String>::new()
    );
    assert_eq!(
        feature_ids_in("Cards (CHG-002)"),
        vec!["CHG-002".to_string()]
    );
    assert_eq!(feature_ids_in("Add IDs (F10)"), vec!["F10".to_string()]);
    assert_eq!(
        feature_ids_in("A (CHG-002) and B (CHG-002)"),
        vec!["CHG-002".to_string()]
    );
    assert_eq!(feature_ids_in("bad CHG-0 short id"), Vec::<String>::new());
    let multi = vec!["CHG-001".to_string(), "CHG-002".to_string()];
    assert_eq!(
        brief_target_problem(&multi, Some("CHG-001"), &|_| true).as_deref(),
        Some(
            "Brief feature name declares more than one feature id (CHG-001, CHG-002); name exactly one"
        )
    );
    let one = vec!["CHG-099".to_string()];
    assert!(
        brief_target_problem(&one, Some("CHG-001"), &|_| true)
            .unwrap()
            .contains("targets CHG-099")
    );
    assert!(
        brief_target_problem(&one, Some("CHG-001"), &|_| false)
            .unwrap()
            .contains("unknown feature CHG-099")
    );
    assert!(
        brief_target_problem(&one, None, &|_| true)
            .unwrap()
            .contains("no feature is active")
    );
    assert!(brief_target_problem(&[], Some("CHG-001"), &|_| false).is_none());
}

#[test]
fn unknown_feature_guidance_uses_the_injected_planning_path() {
    let root = std::env::temp_dir().join(format!(
        "koolade-managed-feature-path-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let message =
        brief_target_problem_with_store(&["F2".into()], None, &|_| false, &store).unwrap();

    assert!(message.contains("planning/changes"));
    assert!(!message.contains(".koolade-packet"));
    let _ = std::fs::remove_dir_all(root);
}
