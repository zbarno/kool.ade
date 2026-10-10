use super::*;

mod schema_three;

#[test]
fn unchanged_legacy_workflow_is_normalized_and_checkpointed() {
    use crate::{
        artifacts::layout::canonical,
        domain::{ArtifactIdentity, ChangeMetadata, ChangeStatus},
    };

    let root = repo("unchanged-workflow");
    let feature_path = root
        .join(canonical::CHANGES)
        .join("CHG-010-saved-searches/specification.md");
    fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
    let feature = ArtifactIdentity::preserve_markdown(
        "# CHG-010: Saved searches\n\n## Intent\n\nPreserve the approved feature.\n",
        None,
        "CHG-010",
        "Saved searches",
    )
    .unwrap();
    let identity = ArtifactIdentity::from_markdown(&feature).unwrap().unwrap();
    let feature = ChangeMetadata::write_markdown(&feature, &identity, ChangeStatus::Ready).unwrap();
    fs::write(feature_path, feature).unwrap();

    let workflow_path = root.join(canonical::WORKFLOW);
    fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
    let legacy_workflow = serde_json::to_vec_pretty(&serde_json::json!({
        "brief": null,
        "reviewedSpecification": null,
        "taskBatches": [],
        "approvedFeatures": {"CHG-010": "frozen approved contract"}
    }))
    .unwrap();
    fs::write(&workflow_path, &legacy_workflow).unwrap();
    commit_all(&root, "legacy workflow without other migration changes");

    let changed = run(&root).unwrap();
    let workflow = crate::artifacts::task_docs::load_workflow(&root).unwrap();
    assert_eq!(
        workflow
            .approved_features
            .get("CHG-010")
            .map(String::as_str),
        Some("frozen approved contract")
    );
    let record_path = root
        .join(canonical::STATE)
        .join("workflow")
        .join(format!("{}.json", workflow.feature_record_ids["CHG-010"]));
    assert!(record_path.is_file());
    assert!(changed.iter().any(|path| path.contains("/state/workflow/")));
    assert_eq!(fs::read(workflow_path).unwrap(), legacy_workflow);
    assert!(run(&root).unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn story_only_batch_gets_durable_batch_identity_and_workflow_entry() {
    use crate::domain::ArtifactIdentity;

    let root = repo("story-only-batch");
    let directory = format!(
        "{}/imported-stories",
        crate::artifacts::layout::canonical::TASKS
    );
    let ticket = format!("{directory}/001-recover-work.md");
    fs::create_dir_all(root.join(&directory)).unwrap();
    fs::write(
        root.join(&ticket),
        "# Recover imported work\n\nPreserve the existing implementation.\n",
    )
    .unwrap();
    commit_all(&root, "story without batch metadata");

    run(&root).unwrap();
    let readme = fs::read_to_string(root.join(&directory).join("README.md")).unwrap();
    let batch_id = ArtifactIdentity::from_markdown(&readme).unwrap().unwrap();
    let task = fs::read_to_string(root.join(&ticket)).unwrap();
    let task_id = ArtifactIdentity::from_markdown(&task).unwrap().unwrap();
    assert_eq!(task_id.parent_uid.as_deref(), Some(batch_id.uid.as_str()));
    let workflow = crate::artifacts::task_docs::load_workflow(&root).unwrap();
    assert_eq!(workflow.task_batches.len(), 1);
    assert_eq!(workflow.task_batches[0].identity.as_ref(), Some(&batch_id));
    assert_eq!(
        crate::artifacts::task_docs::load_board(&root, &workflow).len(),
        1
    );
    assert!(run(&root).unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn duplicate_uids_abort_identity_migration_before_writes() {
    use crate::domain::ArtifactIdentity;

    let root = repo("identity-conflict");
    let shared = ArtifactIdentity::new("F9", "Saved exports");
    let feature_markdown = format!(
        "# F9: Saved exports\n\n**Status:** Ready\n\n<!-- koolade-artifact-id:v1 {} -->\n\n## Intent\n\nExport saved searches.\n\n## Current Behavior\n\nExports are unavailable.\n\n## Desired Behavior\n\nSaved searches can be exported.\n\n## Scope\n\nExport only.\n\n## Affected Product Areas\n\nSearch export.\n\n## Requirements\n\n- Export saved searches.\n\n## Decisions and Assumptions\n\n- Export uses a portable format.\n\n## Acceptance Criteria\n\n- An export can be downloaded.\n",
        serde_json::to_string(&shared).unwrap()
    );
    let feature_path = root
        .join(crate::artifacts::layout::canonical::CHANGES)
        .join("F9-saved-exports/specification.md");
    fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
    fs::write(&feature_path, &feature_markdown).unwrap();

    let batch_dir = format!(
        "{}/F9-saved-exports",
        crate::artifacts::layout::canonical::TASKS
    );
    let batch_path = root.join(&batch_dir);
    fs::create_dir_all(&batch_path).unwrap();
    fs::write(
        batch_path.join("README.md"),
        "# Saved exports — task stories\n",
    )
    .unwrap();
    let task_path = format!("{batch_dir}/001-export.md");
    let mut duplicate = shared;
    duplicate.display_id = "001-export".into();
    duplicate.title = "Export saved searches".into();
    let task_markdown = format!(
        "# Export saved searches\n\n<!-- koolade-artifact-id:v1 {} -->\n\n## Acceptance criteria\n\n- An export can be downloaded.\n",
        serde_json::to_string(&duplicate).unwrap()
    );
    fs::write(root.join(&task_path), &task_markdown).unwrap();
    let workflow_path = root.join(crate::artifacts::layout::canonical::WORKFLOW);
    fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
    fs::write(
        &workflow_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "taskBatches": [{"feature":"Saved exports","directory":batch_dir,"count":1}]
        }))
        .unwrap(),
    )
    .unwrap();
    let manifest_path = root.join(crate::artifacts::layout::canonical::MANIFEST);
    fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    fs::write(&manifest_path, r#"{"schemaVersion":2,"product":"Koolade"}"#).unwrap();
    commit_all(&root, "conflicting identities");

    let error = run(&root).unwrap_err().to_string();
    assert!(error.contains("shared by feature:") && error.contains("task:"));
    assert_eq!(fs::read_to_string(feature_path).unwrap(), feature_markdown);
    assert_eq!(
        fs::read_to_string(root.join(&task_path)).unwrap(),
        task_markdown
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["schemaVersion"], 2);
    assert!(!scoped_pending_path(&root).exists());
    let _ = fs::remove_dir_all(root);
}
