use super::*;

fn init_bootstrap_repo(root: &std::path::Path) {
    std::fs::create_dir_all(root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn archive_relocation_preserves_evidence_and_retries_checkpoint_paths() {
    let root = std::env::temp_dir().join(format!(
        "koolade_archive_relocation_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(root.join("planning")).unwrap();
    let source = crate::artifacts::spec_doc::bootstrap_template("Demo");
    std::fs::write(root.join("planning/specification.md"), &source).unwrap();
    migrate(&root, &source).unwrap();
    std::fs::create_dir_all(root.join("planning/archive")).unwrap();
    std::fs::rename(root.join(LEGACY_ARCHIVE), root.join(OLD_LEGACY_ARCHIVE)).unwrap();
    for args in [vec!["init", "-q"], vec!["add", "."]] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    // A collision must not discard either historical record.
    std::fs::write(root.join(LEGACY_ARCHIVE), "other evidence").unwrap();
    assert!(migrate(&root, &source).is_err());
    assert_eq!(
        std::fs::read_to_string(root.join(OLD_LEGACY_ARCHIVE)).unwrap(),
        source
    );
    assert_eq!(
        std::fs::read_to_string(root.join(LEGACY_ARCHIVE)).unwrap(),
        "other evidence"
    );
    std::fs::remove_file(root.join(LEGACY_ARCHIVE)).unwrap();
    let paths = migrate(&root, &source).unwrap();
    assert!(paths.contains(&OLD_LEGACY_ARCHIVE.to_string()));
    assert!(paths.contains(&LEGACY_ARCHIVE.to_string()));
    assert!(!root.join(OLD_LEGACY_ARCHIVE).exists());
    assert_eq!(
        std::fs::read_to_string(root.join(LEGACY_ARCHIVE)).unwrap(),
        source
    );
    let retry = migrate(&root, &source).unwrap();
    assert!(retry.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bootstrap_uses_compact_core_and_preserves_legacy_thirteen_section_migrations() {
    let root = std::env::temp_dir().join(format!("koolade_product_migrate_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("planning")).unwrap();
    let mut source = "# Demo — Living Technical Specification\n\n".to_owned();
    for section in crate::core::specification::LEGACY_SECTIONS {
        source.push_str(&format!("## {section}\n\nLegacy {section} content.\n\n"));
    }
    std::fs::write(root.join("planning/specification.md"), &source).unwrap();
    let paths = migrate(&root, &source).unwrap();
    assert_eq!(paths.len(), 17);
    assert!(migrate(&root, &source).unwrap().is_empty());
    let documents = load_documents(&root).unwrap().unwrap();
    let parts = documents
        .iter()
        .map(|doc| doc.content.clone())
        .collect::<Vec<_>>();
    assert_eq!(parts.len(), 13);
    assert!(parts[0].contains("Legacy 1. Vision content."));
    assert!(documents.iter().any(|doc| doc.module.id == "13-source-map"));
    assert!(root.join(LEGACY_ARCHIVE).is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fresh_bootstrap_creates_only_the_six_required_concepts() {
    let root = std::env::temp_dir().join(format!(
        "koolade_product_core_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    init_bootstrap_repo(&root);
    crate::artifacts::migration::bootstrap_product(&root, "Compact Demo").unwrap();
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
fn canonical_product_tree_does_not_fall_back_to_legacy_manifest_loading() {
    let root = std::env::temp_dir().join(format!(
        "koolade_product_no_legacy_fallback_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let product = root.join(PRODUCT_DIR);
    std::fs::create_dir_all(&product).unwrap();
    for file in LEGACY_MODULES {
        std::fs::write(product.join(file), format!("# {file}\n\nOld content.\n")).unwrap();
    }
    let error = load_manifest(&root).unwrap_err().to_string();
    assert!(error.contains("manifest is missing"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn migration_preserves_historical_tasks_and_open_board_items() {
    use crate::domain::{ItemKind, OpenItem, Priority};
    let root = std::env::temp_dir().join(format!(
        "koolade_migrate_history_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(root.join("planning/tasks/legacy-batch")).unwrap();
    let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
    std::fs::write(root.join("planning/specification.md"), &legacy).unwrap();
    let task = "# Legacy task\n\nFrozen specification and acceptance.\n";
    let task_path = root.join("planning/tasks/legacy-batch/001-legacy.md");
    let canonical_task = root.join(format!(
        "{}/legacy-batch/001-legacy.md",
        crate::artifacts::layout::canonical::TASKS
    ));
    std::fs::write(&task_path, task).unwrap();
    let item = OpenItem::new(
        "CLR-041".into(),
        Priority::High,
        ItemKind::Question,
        "Product".into(),
        Some("Owner".into()),
        "Which behavior should the legacy task keep?".into(),
        "Requires a product decision.".into(),
    );
    let items_path = root.join("planning/open-items.md");
    let canonical_items = root.join(crate::artifacts::layout::canonical::OPEN_ITEMS);
    let items = crate::artifacts::items_io::serialize(std::slice::from_ref(&item));
    std::fs::write(&items_path, &items).unwrap();
    migrate(&root, &legacy).unwrap();
    assert!(!task_path.exists());
    assert!(!items_path.exists());
    assert_eq!(std::fs::read_to_string(&canonical_task).unwrap(), task);
    assert_eq!(std::fs::read_to_string(&canonical_items).unwrap(), items);
    let restored = crate::artifacts::items_io::parse(&items).unwrap();
    assert_eq!(restored[0], item);
    assert_eq!(restored[0].authority, crate::domain::Authority::Human);
    assert!(
        render_product(&root)
            .unwrap()
            .unwrap()
            .contains("## Quality and Acceptance")
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
    crate::artifacts::migration::bootstrap_product(&root, "ID Test").unwrap();
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
