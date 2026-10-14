use std::{fs, path::Path};

pub(super) fn assert_installed(task_repository: &Path) {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(task_repository.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest["dependencies"]["archive-helper"], "1.0.0");
    let lockfile: serde_json::Value =
        serde_json::from_slice(&fs::read(task_repository.join("package-lock.json")).unwrap())
            .unwrap();
    assert_eq!(
        lockfile["packages"]["node_modules/archive-helper"]["version"],
        "1.0.0"
    );
    assert!(
        task_repository
            .join("node_modules/archive-helper/package.json")
            .is_file()
    );
    assert_eq!(
        fs::read_to_string(task_repository.join("node_modules/archive-helper/index.js")).unwrap(),
        "module.exports = 'ready';\n"
    );
}
