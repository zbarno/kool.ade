use crate::app::session::Project;
use crate::core::self_improvement::SelfImprovementRunV1;

pub(super) fn persist_task_document(
    project: &mut Project,
    run: &SelfImprovementRunV1,
    task_uid: &str,
    feature_id: &str,
) -> String {
    let identity = crate::domain::ArtifactIdentity {
        uid: task_uid.into(),
        display_id: format!("TASK-{}", task_uid[..8].to_uppercase()),
        title: "Execute the approved lifecycle task".into(),
        parent_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    };
    let branch_targets = crate::core::workflow::BranchTargets {
        source: run.source_ref.clone().expect("approved source ref"),
        destination: run
            .destination_ref
            .clone()
            .expect("approved destination ref"),
    };
    let git_intent = crate::core::git_intent::GitIntentV1::approve(
        "root".into(),
        branch_targets.source.clone(),
        run.source_sha.clone(),
        branch_targets.destination.clone(),
        crate::core::git_intent::PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap();
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_branch_targets(Some(&branch_targets))
        .unwrap()
        .with_source_sha(Some(&run.source_sha))
        .unwrap()
        .with_git_intent(Some(&git_intent))
        .unwrap();
    let text = format!(
        "# Execute the approved lifecycle task\n\nFeature: {feature_id}\n\n## Acceptance Criteria\n\n- The local worker output exists.\n"
    );
    let seeded_identity = format!(
        "<!-- koolade-artifact-id:v1 {} -->\n",
        serde_json::to_string(&identity).unwrap()
    );
    let identified_text = crate::domain::ArtifactIdentity::preserve_markdown(
        &text,
        Some(&seeded_identity),
        &identity.display_id,
        &identity.title,
    )
    .unwrap();
    let path = "planning/tasks/self-improvement/001-task.md";
    let encoded = crate::artifacts::task_docs::embed(&identified_text, &metadata).unwrap();
    project
        .state
        .planning_store
        .atomic_write(path, encoded.as_bytes())
        .unwrap();

    let mut batch_identity = crate::domain::ArtifactIdentity::new(
        &format!("BATCH-{}", task_uid[..8].to_uppercase()),
        "Approved self-improvement task batch",
    );
    batch_identity.parent_uid = Some(
        crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
            .load_task(task_uid)
            .unwrap()
            .feature_uid,
    );
    let readme = crate::domain::ArtifactIdentity::preserve_markdown(
        "# Approved self-improvement task batch\n\nStatus: Complete — all stories and batch checks validated.\n",
        Some(&format!(
            "<!-- koolade-artifact-id:v1 {} -->\n",
            serde_json::to_string(&batch_identity).unwrap()
        )),
        &batch_identity.display_id,
        &batch_identity.title,
    )
    .unwrap();
    project
        .state
        .planning_store
        .atomic_write(
            "planning/tasks/self-improvement/README.md",
            readme.as_bytes(),
        )
        .unwrap();
    project
        .state
        .workflow
        .task_batches
        .push(crate::core::workflow::TaskBatchRef {
            identity: Some(batch_identity),
            feature: format!("Feature {feature_id}"),
            directory: "planning/tasks/self-improvement".into(),
            count: 1,
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        });
    crate::artifacts::task_docs::save_workflow(
        &project.state.planning_store,
        &project.state.workflow,
    )
    .unwrap();
    project.task_documents = crate::artifacts::task_docs::load_board(
        &project.state.planning_store,
        &project.state.workflow,
    );
    path.to_owned()
}
