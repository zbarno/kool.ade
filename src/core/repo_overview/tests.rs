use super::*;

#[test]
fn scans_shape_of_repo() {
    let tmp = std::env::temp_dir().join(format!("koolade_ov_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("src/deep/deeper/deepest")).unwrap();
    fs::create_dir_all(tmp.join("node_modules/pkg")).unwrap();
    fs::write(tmp.join("README.md"), "# Hi\nWorld").unwrap();
    fs::write(tmp.join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.join("src/lib.rs"), "//").unwrap();
    fs::write(tmp.join("node_modules/pkg/index.js"), "").unwrap();
    let imports = crate::artifacts::layout::ArtifactLayout::new(&tmp).imports_root();
    fs::create_dir_all(&imports).unwrap();
    fs::write(imports.join("doc.txt"), "ref").unwrap();

    let ov = scan(&tmp);
    assert!(ov.readme.as_deref().unwrap_or("").contains("# Hi"));
    assert!(ov.manifests.contains(&"cargo.toml".to_string()));
    assert!(ov.tree_lines.iter().any(|l| l == "src/"));
    assert!(!ov.tree_lines.iter().any(|l| l.starts_with("node_modules")));
    assert!(!ov.tree_lines.iter().any(|l| l.contains("deepest")));
    assert!(
        ov.planning_files
            .iter()
            .any(|l| l.contains(".koolade-packet/planning/imports/doc.txt (0 KB)"))
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn managed_store_scan_excludes_stale_embedded_planning_artifacts() {
    let tmp = std::env::temp_dir().join(format!(
        "koolade_managed_overview_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let repo = tmp.join("code");
    let store_root = tmp.join("planning");
    fs::create_dir_all(repo.join(".koolade-packet/planning/product")).unwrap();
    fs::create_dir_all(&store_root).unwrap();
    fs::write(
        repo.join(".koolade-packet/planning/product/index.md"),
        "stale embedded spec",
    )
    .unwrap();
    fs::write(repo.join("src.rs"), "source").unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        store_root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );

    let overview = scan_for_store(&repo, &store);

    assert!(overview.tree_lines.iter().any(|line| line == "src.rs"));
    assert!(
        overview
            .tree_lines
            .iter()
            .all(|line| !line.starts_with(".koolade-packet"))
    );
    assert!(overview.planning_files.is_empty());
    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn readme_truncation_caps_length() {
    let tmp = std::env::temp_dir().join(format!("koolade_rd_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("README.md"), "x".repeat(9000)).unwrap();
    let rd = find_readme(&tmp).unwrap();
    assert!(rd.chars().count() <= MAX_README_CHARS + 5);
    assert!(rd.ends_with("…[truncated]"));
    let _ = fs::remove_dir_all(&tmp);
}
