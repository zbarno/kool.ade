use super::{local_root_for_workspace, workspace_root};

#[test]
fn local_evidence_storage_is_persistent_and_workspace_scoped() {
    let workspace_id = uuid::Uuid::new_v4();
    let root = local_root_for_workspace(workspace_id).unwrap();

    assert!(root.is_absolute());
    assert_eq!(
        root.file_name().unwrap().to_str().unwrap(),
        workspace_id.hyphenated().to_string()
    );
    assert_eq!(root.parent().unwrap().file_name().unwrap(), "runtime-qa");

    let base = std::path::Path::new("/user-data");
    assert_eq!(
        workspace_root(base, workspace_id),
        base.join("koolade/runtime-qa")
            .join(workspace_id.hyphenated().to_string())
    );
    assert_ne!(
        workspace_root(base, workspace_id),
        workspace_root(base, uuid::Uuid::new_v4())
    );
}
