use crate::{
    artifacts::{planning_store::PlanningStore, task_docs::TaskMetadata},
    domain::{ArtifactIdentity, ChangeMetadata, ChangeStatus},
};

use super::fixtures::{feature_markdown, identity_markdown};

pub struct SecondaryFeature {
    pub feature: ArtifactIdentity,
    pub batch: ArtifactIdentity,
    pub task: ArtifactIdentity,
}

pub fn add(store: &PlanningStore) -> SecondaryFeature {
    let feature = ArtifactIdentity::new("F2", "Refund export");
    let feature_text = feature_markdown(&feature);
    store
        .atomic_write(
            "planning/changes/F2-refund-export/specification.md",
            feature_text.as_bytes(),
        )
        .unwrap();
    store
        .atomic_write(
            "planning/changes/F2-refund-export/metadata.json",
            &serde_json::to_vec_pretty(&ChangeMetadata::new(&feature, ChangeStatus::Ready))
                .unwrap(),
        )
        .unwrap();

    let batch = ArtifactIdentity {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_id: "BATCH-002".into(),
        title: "Refund export tasks".into(),
        parent_uid: Some(feature.uid.clone()),
    };
    let task = ArtifactIdentity {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_id: "TASK-002".into(),
        title: "Add refund export".into(),
        parent_uid: Some(batch.uid.clone()),
    };
    let metadata = TaskMetadata::new(&task, "root", Vec::new()).unwrap();
    let task_md = format!(
        "---\nkoolade-task: {}\n---\n\n{}",
        serde_json::to_string(&metadata).unwrap(),
        identity_markdown(&task, "# TASK-002: Add refund export\n\nExport refunds.\n")
    );
    store
        .atomic_write(
            "planning/tasks/F2-refund-export/README.md",
            identity_markdown(&batch, "# Refund export tasks\n").as_bytes(),
        )
        .unwrap();
    store
        .atomic_write(
            "planning/tasks/F2-refund-export/specification.md",
            feature_text.as_bytes(),
        )
        .unwrap();
    store
        .atomic_write(
            "planning/tasks/F2-refund-export/001-refund-export.md",
            task_md.as_bytes(),
        )
        .unwrap();
    SecondaryFeature {
        feature,
        batch,
        task,
    }
}
