use super::*;
use crate::core::git_intent::{GitIntentV1, PublicationModeV1};

fn record_path(root: &std::path::Path, item: &Work) -> std::path::PathBuf {
    root.join(format!("state/work/{}.json", item.uid))
}

fn seed_legacy(
    root: &std::path::Path,
    item: &Work,
    change: impl FnOnce(&mut serde_json::Value),
) -> Vec<u8> {
    let path = record_path(root, item);
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    change(&mut record);
    let bytes = serde_json::to_vec_pretty(&record).unwrap();
    std::fs::write(path, &bytes).unwrap();
    bytes
}

fn backup_path(root: &std::path::Path, item: &Work) -> std::path::PathBuf {
    root.join(format!(
        "state/recovery/unmapped-primary-git-intent/{}-revision-1.json",
        item.uid
    ))
}

#[test]
fn legacy_branch_names_are_backed_up_and_never_become_git_approval() {
    let root = super::root();
    let work = Work::new(
        "task:unapproved".into(),
        "Legacy branch-only work".into(),
        "Preserve my request".into(),
        "Preserve my detail".into(),
    );
    save(&root, std::slice::from_ref(&work)).unwrap();
    let original = seed_legacy(&root, &work, |record| {
        record["data"]["sourceBranch"] = "main".into();
        record["data"]["destinationBranch"] = "release".into();
    });
    assert!(
        load(&root)
            .unwrap_err()
            .to_string()
            .contains("Unmapped primary repository Git intent")
    );

    assert_eq!(recover_unmapped_primary_git_intents(&root).unwrap(), 1);
    assert_eq!(std::fs::read(backup_path(&root, &work)).unwrap(), original);
    let restored = load(&root).unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].request, work.request);
    assert_eq!(restored[0].detail, work.detail);
    assert!(restored[0].git_intents.is_empty());
    assert!(restored[0].git_intent.is_none());
    assert!(restored[0].source_branch.is_none());
    assert!(restored[0].destination_branch.is_none());
    assert_eq!(restored[0].record_revision, 2);
    assert_eq!(recover_unmapped_primary_git_intents(&root).unwrap(), 0);
    assert_eq!(std::fs::read(backup_path(&root, &work)).unwrap(), original);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_valid_legacy_approval_is_recovered_without_minting_a_new_approval() {
    let root = super::root();
    let mut work = Work::new(
        "task:approved".into(),
        "Legacy approved work".into(),
        "Original approved request".into(),
        String::new(),
    );
    let intent = GitIntentV1::approve(
        "root".into(),
        "main".into(),
        "a".repeat(40),
        "release".into(),
        PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap();
    work.git_intent = Some(intent.clone());
    work.git_intents.insert("root".into(), intent.clone());
    work.source_branch = Some(intent.source_ref.clone());
    work.destination_branch = Some(intent.destination_ref.clone());
    save(&root, std::slice::from_ref(&work)).unwrap();

    let original = seed_legacy(&root, &work, |record| {
        record["data"].as_object_mut().unwrap().remove("gitIntents");
    });
    assert_eq!(recover_unmapped_primary_git_intents(&root).unwrap(), 1);
    assert_eq!(std::fs::read(backup_path(&root, &work)).unwrap(), original);
    let restored = load(&root).unwrap();
    assert_eq!(restored[0].git_intents.get("root"), Some(&intent));
    assert_eq!(restored[0].git_intent.as_ref(), Some(&intent));
    assert_eq!(restored[0].source_branch.as_deref(), Some("main"));
    assert_eq!(restored[0].destination_branch.as_deref(), Some("release"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_invalid_legacy_approval_is_quarantined_not_implicitly_authorized() {
    let root = super::root();
    let work = Work::new(
        "task:invalid-approval".into(),
        "Corrupt legacy approval".into(),
        "Do not authorize this".into(),
        String::new(),
    );
    save(&root, std::slice::from_ref(&work)).unwrap();
    let mut intent = GitIntentV1::approve(
        "root".into(),
        "main".into(),
        "b".repeat(40),
        "main".into(),
        PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap();
    intent.approval_revision = "invalid".into();
    let original = seed_legacy(&root, &work, |record| {
        record["data"]["gitIntent"] = serde_json::to_value(&intent).unwrap();
        record["data"]["sourceBranch"] = "main".into();
        record["data"]["destinationBranch"] = "main".into();
    });

    assert_eq!(recover_unmapped_primary_git_intents(&root).unwrap(), 1);
    assert_eq!(std::fs::read(backup_path(&root, &work)).unwrap(), original);
    let restored = load(&root).unwrap();
    assert!(restored[0].git_intent.is_none());
    assert!(restored[0].git_intents.is_empty());
    assert!(restored[0].source_branch.is_none());
    assert!(restored[0].destination_branch.is_none());
    std::fs::remove_dir_all(root).unwrap();
}
