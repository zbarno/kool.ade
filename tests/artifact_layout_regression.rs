use std::{fs, path::Path};

use koolade::{artifacts::layout::canonical, core::planning_work};

#[test]
fn koolade_repository_keeps_runtime_artifacts_in_the_canonical_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in ["planning", ".planner", "adr", "SPECIFICATION.md"] {
        assert!(
            !root.join(path).exists(),
            "legacy runtime artifact remains outside .koolade: {path}"
        );
    }
    assert!(
        !root
            .join(koolade::artifacts::layout::previous::WORK)
            .exists(),
        "planning work remains in its previous canonical location"
    );

    for path in [
        canonical::MANIFEST,
        canonical::PROJECT_CONFIG,
        canonical::PROJECT_MANIFEST,
        canonical::PRODUCT_MANIFEST,
        canonical::OPEN_ITEMS,
        canonical::RESOLVED_ITEMS,
        canonical::WORKFLOW,
        canonical::WORK,
    ] {
        assert!(
            root.join(path).is_file(),
            "missing canonical artifact: {path}"
        );
    }
    assert!(root.join(canonical::CHANGES).is_dir());
    let product: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(canonical::PRODUCT_MANIFEST)).unwrap()).unwrap();
    for module in product["modules"].as_array().unwrap() {
        let path = root
            .join(canonical::PRODUCT)
            .join(module["path"].as_str().unwrap());
        assert!(
            path.is_file(),
            "product module is missing: {}",
            path.display()
        );
    }
}

#[test]
fn planning_work_save_creates_the_canonical_state_directory() {
    let root = std::env::temp_dir().join(format!(
        "koolade-work-state-layout-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&root).unwrap();
    planning_work::save(&root, &[]).unwrap();
    assert!(root.join(canonical::WORK).is_file());
    assert!(root.join(canonical::STATE).is_dir());
    let _ = fs::remove_dir_all(root);
}
