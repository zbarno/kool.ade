use super::*;
use crate::domain::{
    ArtifactIdentity, ChangeMetadata, ChangeStatus, ItemKind, Priority, item::OpenItem,
};

#[test]
fn applied_child_refreshes_only_its_unblocked_identity_matched_parent() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success()
    );
    let mut before = PlannerState::load(dir.path()).unwrap();
    let body = ArtifactIdentity::preserve_markdown(
        "# F1: Example\n\n**Status:** Draft\n",
        None,
        "F1",
        "Example",
    )
    .unwrap();
    let identity = ArtifactIdentity::from_markdown(&body).unwrap().unwrap();
    let body = ChangeMetadata::write_markdown(&body, &identity, ChangeStatus::Draft).unwrap();
    before.active_features.push(("F1".into(), body));
    let mut child = OpenItem::new(
        "CLR-001".into(),
        Priority::High,
        ItemKind::Question,
        "General".into(),
        None,
        "Which store?".into(),
        "Need a decision".into(),
    );
    child.feature_id = Some("F1".into());
    child.feature_uid = Some(identity.uid.clone());
    let key = child.conversation_key().to_owned();
    before.items.push(child.clone());
    let mut parent = Work::new(
        "planning:parent".into(),
        "Plan Example".into(),
        "Plan Example".into(),
        "Waiting on CLR-001".into(),
    );
    parent.feature_id = Some("F1".into());
    parent.feature_uid = Some(identity.uid.clone());
    parent.status = WorkStatus::NeedsAttention;
    let mut reused = parent.clone();
    reused.uid = uuid::Uuid::new_v4().to_string();
    reused.key = "planning:reused".into();
    reused.feature_uid = Some(uuid::Uuid::new_v4().to_string());
    let mut failed = parent.clone();
    failed.key = "planning:failed".into();
    let mut work = vec![parent, reused.clone(), failed.clone()];
    let excluded = std::collections::BTreeSet::from([failed.key.clone()]);
    let mut after = before.clone();
    after.items.clear();
    assert!(reconcile_answered_child(
        &before, &after, &key, &mut work, &excluded
    ));
    assert_eq!(work[0].status, WorkStatus::InReview);
    assert!(!work[0].detail.contains("Waiting on CLR-001"));
    assert_eq!(work[1], reused);
    assert_eq!(work[2], failed);
    child.id = "CLR-002".into();
    child.authority = crate::domain::Authority::Agent;
    after.items.push(child);
    assert!(reconcile_answered_child(
        &before, &after, &key, &mut work, &excluded
    ));
    assert_eq!(work[0].status, WorkStatus::NeedsAttention);
    assert!(work[0].detail.contains("CLR-002"));
    after.items.clear();
    after.active_features[0].1 = ChangeMetadata::write_markdown(
        &after.active_features[0].1,
        &identity,
        ChangeStatus::Implementing,
    )
    .unwrap();
    assert!(!reconcile_answered_child(
        &before, &after, &key, &mut work, &excluded
    ));
    assert_eq!(
        work[0].status,
        WorkStatus::NeedsAttention,
        "later implementation work cannot be sent back to planning review"
    );
    after.active_features.clear();
    assert!(!reconcile_answered_child(
        &before, &after, &key, &mut work, &excluded
    ));
}
