use super::*;
use crate::artifacts::planning_store::StoreMode;
use std::fs;

#[test]
fn managed_bootstrap_ignores_legacy_features_until_they_are_migrated() {
    let root = std::env::temp_dir().join(format!(
        "koolade-managed-product-bootstrap-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let code_root = root.join("code");
    let planning_root = root.join("planning");
    let old_feature = code_root.join("planning/features/CHG-001-old-code-plan/specification.md");
    fs::create_dir_all(old_feature.parent().unwrap()).unwrap();
    fs::write(
        old_feature,
        "# CHG-001: Old code plan\n\n**Status:** Implemented\n\n## Intent\n\nOld checkout data.\n",
    )
    .unwrap();
    fs::create_dir_all(&planning_root).unwrap();
    let old_managed_feature =
        planning_root.join("planning/features/CHG-002-old-managed-plan/specification.md");
    fs::create_dir_all(old_managed_feature.parent().unwrap()).unwrap();
    fs::write(
        old_managed_feature,
        "# CHG-002: Old managed plan\n\n**Status:** Draft\n\n## Intent\n\nNeeds migration.\n",
    )
    .unwrap();
    let current_feature = planning_root.join("planning/changes/CHG-003-current/specification.md");
    fs::create_dir_all(current_feature.parent().unwrap()).unwrap();
    fs::write(
        current_feature,
        "# CHG-003: Current plan\n\n**Status:** Draft\n\n## Intent\n\nCurrent store data.\n",
    )
    .unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning_root,
        StoreMode::ManagedLocal,
    );

    let files = bootstrap_files_with_store(&code_root, &store, None).unwrap();
    let index = files
        .iter()
        .find(|file| file.target == canonical::PRODUCT_INDEX)
        .unwrap();
    let index = std::str::from_utf8(&index.bytes).unwrap();
    let active_features = index.split("## Active features\n\n").nth(1).unwrap();
    assert!(
        active_features
            .contains("[`CHG-003-current`](../changes/CHG-003-current/specification.md)")
    );
    assert!(!index.contains("CHG-001-old-code-plan"));
    assert!(!index.contains("CHG-002-old-managed-plan"));

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn managed_bootstrap_rejects_a_symlinked_legacy_specification() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "koolade-managed-product-symlink-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let code_root = root.join("code");
    let planning_root = root.join("planning");
    let outside = root.join("outside.md");
    fs::create_dir_all(planning_root.join("planning")).unwrap();
    fs::write(
        &outside,
        "# Outside specification\n\n## Vision\n\nMust not be imported.\n",
    )
    .unwrap();
    symlink(&outside, planning_root.join("planning/specification.md")).unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning_root,
        StoreMode::ManagedLocal,
    );

    let error = bootstrap_files_with_store(&code_root, &store, None).unwrap_err();
    assert!(error.to_string().contains("invalid planning path"));
    assert_eq!(
        fs::read_to_string(outside).unwrap(),
        "# Outside specification\n\n## Vision\n\nMust not be imported.\n"
    );
    assert!(!planning_root.join("planning/product/index.md").exists());

    let _ = fs::remove_dir_all(root);
}
