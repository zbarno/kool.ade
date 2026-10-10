use super::*;

#[test]
fn feature_ids_continue_past_three_digits() {
    let root = std::env::temp_dir().join(format!(
        "koolade_feature_ids_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(root.join(format!(
        "{}/CHG-1000-old",
        crate::artifacts::layout::canonical::CHANGES
    )))
    .unwrap();
    std::fs::write(
        root.join(format!(
            "{}/CHG-1000-old/specification.md",
            crate::artifacts::layout::canonical::CHANGES
        )),
        "# CHG-1000: Old\n\n**Status:** Draft\n",
    )
    .unwrap();
    crate::artifacts::product_docs::migrate_legacy_change_fixtures(&root).unwrap();
    assert_eq!(next_feature_id(&root), "F1001");
    assert!(document_path(&root, "feature:CHG-1000").unwrap().exists());
    assert_eq!(active_feature(&root).unwrap().0, "CHG-1000");
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn every_nonterminal_feature_is_active_concurrently() {
    let root = std::env::temp_dir().join(format!(
        "koolade_active_features_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    for (directory, status) in [
        ("CHG-001-first", "Ready"),
        ("CHG-002-second", "Implementing"),
        ("CHG-003-done", "Implemented"),
    ] {
        std::fs::create_dir_all(
            root.join(crate::artifacts::layout::canonical::CHANGES)
                .join(directory),
        )
        .unwrap();
        std::fs::write(
            root.join(crate::artifacts::layout::canonical::CHANGES)
                .join(directory)
                .join("specification.md"),
            format!(
                "# {}: Feature\n\n**Status:** {status}\n",
                directory.split('-').take(2).collect::<Vec<_>>().join("-")
            ),
        )
        .unwrap();
    }
    crate::artifacts::product_docs::migrate_legacy_change_fixtures(&root).unwrap();
    let features = active_features(&root);
    assert_eq!(
        features
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        vec!["CHG-001", "CHG-002"]
    );
    assert_eq!(active_feature(&root).unwrap().0, "CHG-001");
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn explicit_index_edits_cannot_forge_active_feature_manifest() {
    let root = std::env::temp_dir().join(format!(
        "koolade_index_manifest_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    init_bootstrap_repo(&root);
    crate::artifacts::migration::bootstrap_product(&root, "Index Demo").unwrap();
    std::fs::create_dir_all(root.join(format!(
        "{}/CHG-001-first",
        crate::artifacts::layout::canonical::CHANGES
    )))
    .unwrap();
    std::fs::write(
        root.join(format!(
            "{}/CHG-001-first/specification.md",
            crate::artifacts::layout::canonical::CHANGES
        )),
        "# CHG-001: First\n\n**Status:** Draft\n",
    )
    .unwrap();
    crate::artifacts::product_docs::migrate_legacy_change_fixtures(&root).unwrap();
    let forged =
        "# Revised product\n\n## Modules\n\nExisting modules.\n\n## Active features\n\n- fake\n";
    let actual = refreshed_index_from(&root, forged, &[]).unwrap();
    assert!(actual.contains("CHG-001-first"));
    assert!(!actual.contains("- fake"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn optional_product_modules_have_manifest_order_and_render_without_code_changes() {
    let root = std::env::temp_dir().join(format!(
        "koolade_optional_product_module_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    init_bootstrap_repo(&root);
    crate::artifacts::migration::bootstrap_product(&root, "Growing Demo").unwrap();
    let update = vec![(
        "product:billing".to_owned(),
        "# Billing\n\nThe project uses an external billing provider.\n".to_owned(),
    )];
    let path = document_path_for_update(&root, &update[0].0, &update[0].1).unwrap();
    let manifest = updated_manifest(&root, &update).unwrap();
    assert_eq!(manifest.modules.last().unwrap().id, "billing");
    crate::artifacts::atomic_write(&path, &update[0].1).unwrap();
    crate::artifacts::atomic_write(
        &crate::artifacts::layout::ArtifactLayout::new(&root).product_manifest(),
        &serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let docs = load_documents(&root).unwrap().unwrap();
    assert_eq!(docs.last().unwrap().module.title, "Billing");
    assert!(
        render_product(&root)
            .unwrap()
            .unwrap()
            .contains("## Billing\n\nThe project uses an external billing provider.")
    );
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn stable_definitions_survive_rewrite_without_freezing_incidental_references() {
    let old = "## 5. Requirements\n\n- **FR-1** Maintain current truth. See D-17.\n";
    let revised = "## 5. Requirements\n\n- **FR-1** Maintain current truth in modules.\n";
    assert!(preserved_ids(old, revised).is_ok());
    assert!(preserved_ids(old, "## 5. Requirements\n\nNo requirements.\n").is_err());
}
