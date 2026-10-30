use super::*;

#[test]
fn canonical_paths_are_rooted_and_logical_children_cannot_escape() {
    let root = PathBuf::from("/repo");
    let layout = ArtifactLayout::new(&root);
    assert_eq!(layout.product_index(), root.join(store::PRODUCT_INDEX));
    assert_eq!(layout.workflow_state(), root.join(store::WORKFLOW));
    assert!(
        layout
            .canonical_path("planning/tasks")
            .unwrap()
            .starts_with(layout.koolade_root())
    );
    assert_eq!(
        layout.change_specification("CHG-007").unwrap(),
        root.join(store::CHANGES).join("CHG-007/specification.md")
    );
    assert_eq!(
        layout.decision_record("ADR-009.md").unwrap(),
        root.join(store::DECISIONS).join("ADR-009.md")
    );
    assert!(layout.task_batch("../outside").is_none());
    for invalid in ["../outside", "/absolute", "planning/../../outside", ""] {
        assert!(
            layout.canonical_path(invalid).is_none(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn task_root_uses_only_the_canonical_tree() {
    let root = std::env::temp_dir().join(format!("koolade-layout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(legacy::TASKS)).unwrap();
    let layout = ArtifactLayout::new(&root);
    assert_eq!(layout.tasks_root(), root.join(store::TASKS));
    std::fs::remove_dir_all(root.join(legacy::TASKS)).unwrap();
    assert_eq!(layout.tasks_root(), root.join(store::TASKS));
    let _ = std::fs::remove_dir_all(root);
}
