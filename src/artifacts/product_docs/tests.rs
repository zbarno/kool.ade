use super::*;

fn init_bootstrap_repo(root: &std::path::Path) {
    std::fs::create_dir_all(root).unwrap();
}

#[test]
fn fresh_bootstrap_creates_only_the_six_required_concepts() {
    let root = std::env::temp_dir().join(format!(
        "koolade_product_core_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    init_bootstrap_repo(&root);
    crate::artifacts::product_bootstrap::bootstrap_product(&root, "Compact Demo").unwrap();
    let documents = load_documents(&root).unwrap().unwrap();
    assert_eq!(documents.len(), 6);
    assert!(
        documents
            .iter()
            .all(|doc| doc.module.core_concept.is_some())
    );
    assert!(
        render_product(&root)
            .unwrap()
            .unwrap()
            .contains("## Quality and Acceptance")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn old_product_modules_without_a_manifest_are_rejected_and_left_untouched() {
    let root = std::env::temp_dir().join(format!(
        "koolade_product_no_legacy_fallback_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let product = root.join(PRODUCT_DIR);
    std::fs::create_dir_all(&product).unwrap();
    let old_modules = [
        "01-vision.md",
        "02-scope.md",
        "03-actors-and-roles.md",
        "04-feature-inventory.md",
        "05-functional-requirements.md",
        "06-non-functional-requirements.md",
        "07-data-model.md",
        "08-architecture.md",
        "09-environment.md",
        "10-decisions.md",
        "11-risks.md",
        "12-acceptance.md",
        "13-source-map.md",
    ];
    for file in old_modules {
        std::fs::write(product.join(file), format!("# {file}\n\nOld content.\n")).unwrap();
    }
    let error = load_manifest(&root).unwrap_err().to_string();
    assert!(error.contains("manifest is missing"));
    assert!(old_modules.iter().all(|file| product.join(file).is_file()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fresh_bootstrap_does_not_import_a_monolithic_specification() {
    let root = std::env::temp_dir().join(format!(
        "koolade_product_seed_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    init_bootstrap_repo(&root);
    let seed = "# Old specification\n\n## Vision\n\nOld content must remain untouched.\n";
    std::fs::create_dir_all(root.join("planning")).unwrap();
    std::fs::write(root.join("planning/specification.md"), seed).unwrap();
    crate::artifacts::product_bootstrap::bootstrap_product(&root, "Fresh Demo").unwrap();
    let product = render_product(&root).unwrap().unwrap();
    assert!(product.contains("Describe the project in chat"));
    assert!(!product.contains("Old content must remain untouched"));
    assert_eq!(
        std::fs::read_to_string(root.join("planning/specification.md")).unwrap(),
        seed
    );
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn logical_ids_cannot_escape_allowlisted_paths() {
    let root = std::env::temp_dir().join(format!(
        "koolade_product_ids_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    init_bootstrap_repo(&root);
    crate::artifacts::product_bootstrap::bootstrap_product(&root, "ID Test").unwrap();
    assert_eq!(
        document_path(&root, "product:architecture-and-constraints").unwrap(),
        root.join(PRODUCT_DIR)
            .join("architecture-and-constraints.md")
    );
    for id in [
        "product:../../etc/passwd",
        "feature:CHG-001/../../etc",
        "product:not-registered",
    ] {
        assert!(document_path(&root, id).is_err());
    }
    let _ = std::fs::remove_dir_all(root);
}
#[path = "tests/active_features.rs"]
mod active_features;
