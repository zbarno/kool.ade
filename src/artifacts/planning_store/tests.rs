use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("koolade-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store(root: PathBuf) -> PlanningStore {
    PlanningStore::new(uuid::Uuid::new_v4(), root, StoreMode::ManagedLocal)
}

#[test]
fn separate_store_roots_resolve_the_same_artifact_independently() {
    let first = TempDir::new();
    let second = TempDir::new();
    let a = store(first.path().join("planning-repo"));
    let b = store(second.path().join("planning-repo"));

    a.atomic_write("planning/product/index.md", b"A").unwrap();
    b.atomic_write("planning/product/index.md", b"B").unwrap();

    assert_eq!(a.read("planning/product/index.md").unwrap(), b"A");
    assert_eq!(b.read("planning/product/index.md").unwrap(), b"B");
    assert_eq!(
        a.layout().product_index(),
        a.root.join("planning/product/index.md")
    );
}

#[test]
fn store_paths_reject_parent_absolute_and_windows_paths() {
    let temp = TempDir::new();
    let store = store(temp.path().join("planning-repo"));
    let absolute = temp.path().join("outside");

    for path in [
        "../outside",
        "planning/../../outside",
        "C:\\outside",
        "planning\\items",
    ] {
        assert!(matches!(store.read(path), Err(StoreError::InvalidPath(_))));
    }
    assert!(matches!(
        store.read(absolute),
        Err(StoreError::InvalidPath(_))
    ));
}

#[cfg(unix)]
#[test]
fn store_paths_reject_symlink_jumps() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new();
    let outside = temp.path().join("outside");
    let root = temp.path().join("planning-repo");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    symlink(&outside, root.join("planning")).unwrap();
    let store = store(root);

    assert!(matches!(
        store.atomic_write("planning/item.md", b"content"),
        Err(StoreError::InvalidPath(_))
    ));
    assert!(!outside.join("item.md").exists());
}

#[cfg(unix)]
#[test]
fn legacy_store_root_symlinks_are_preserved_and_rejected() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new();
    let project = temp.path().join("code");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let root = project.join(crate::artifacts::layout::canonical::ROOT);
    symlink(&outside, &root).unwrap();

    let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &project);
    assert_eq!(store.root, root);
    assert!(matches!(
        store.read("planning/open-items.md"),
        Err(StoreError::InvalidPath(_))
    ));
    assert!(matches!(store.recover(), Err(StoreError::InvalidPath(_))));
    assert!(matches!(store.revision(), Err(StoreError::InvalidPath(_))));
    assert!(std::fs::read_dir(outside).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn dangling_store_root_symlink_is_not_treated_as_an_empty_store() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new();
    let project = temp.path().join("code");
    std::fs::create_dir_all(&project).unwrap();
    let root = project.join(crate::artifacts::layout::canonical::ROOT);
    symlink(temp.path().join("missing-target"), &root).unwrap();

    let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &project);
    assert_eq!(store.root, root);
    assert!(matches!(store.revision(), Err(StoreError::InvalidPath(_))));
}

#[test]
fn compatibility_store_keeps_existing_embedded_root() {
    let temp = TempDir::new();
    let project = temp.path().join("code");
    std::fs::create_dir_all(project.join(".koolade-packet/planning")).unwrap();
    std::fs::write(
        project.join(".koolade-packet/planning/product.md"),
        b"legacy bytes",
    )
    .unwrap();
    let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &project);

    assert_eq!(
        store.root,
        project.join(crate::artifacts::layout::canonical::ROOT)
    );
    assert_eq!(store.read("planning/product.md").unwrap(), b"legacy bytes");
    assert_eq!(
        store.layout().legacy_path("planning/product.md"),
        Some(project.join("planning/product.md"))
    );
}

#[test]
fn relative_store_roots_are_anchored_to_the_creation_directory() {
    struct RestoreCurrentDir(PathBuf);
    impl Drop for RestoreCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    let original = std::env::current_dir().unwrap();
    let restore = RestoreCurrentDir(original.clone());
    let relative = PathBuf::from(format!(".koolade-relative-store-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &relative, StoreMode::ManagedLocal);
    assert!(store.root.is_absolute());
    std::fs::create_dir_all(&store.root).unwrap();
    store
        .atomic_write("planning/specification.md", b"anchored")
        .unwrap();
    let other_directory = TempDir::new();
    std::env::set_current_dir(other_directory.path()).unwrap();
    assert_eq!(
        store.read("planning/specification.md").unwrap(),
        b"anchored"
    );
    drop(restore);
    std::fs::remove_dir_all(original.join(relative)).unwrap();
}

#[test]
fn optimistic_transaction_rejects_a_stale_revision() {
    let temp = TempDir::new();
    let root = temp.path().join("planning-repo");
    std::fs::create_dir_all(&root).unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap();
    let store = store(root);
    let revision = store.revision().unwrap();
    store
        .atomic_write("planning/specification.md", b"another writer")
        .unwrap();

    let result = store.transaction(
        &[("planning/specification.md".into(), b"stale update".to_vec())],
        Some(&revision),
    );
    assert!(matches!(result, Err(StoreError::StaleRevision { .. })));
    assert_eq!(
        store.read("planning/specification.md").unwrap(),
        b"another writer"
    );
}

#[test]
fn different_code_checkouts_share_one_explicit_store_without_branch_paths() {
    let temp = TempDir::new();
    let planning = store(temp.path().join("planning-repo"));
    let code_a = temp.path().join("code-a");
    let code_b = temp.path().join("code-b");
    std::fs::create_dir_all(&code_a).unwrap();
    std::fs::create_dir_all(&code_b).unwrap();
    for root in [&code_a, &code_b] {
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap();
    }
    planning
        .atomic_write(
            "planning/changes/F1/specification.md",
            b"same planning data",
        )
        .unwrap();

    let root_a = std::fs::canonicalize(&code_a).unwrap();
    let root_b = std::fs::canonicalize(&code_b).unwrap();
    assert_ne!(root_a, root_b);
    assert_eq!(
        planning
            .read("planning/changes/F1/specification.md")
            .unwrap(),
        b"same planning data"
    );
    assert_eq!(planning.layout().root(), planning.root);
}

#[test]
fn planning_store_survives_switching_code_checkout_branches() {
    let temp = TempDir::new();
    let code = temp.path().join("code");
    let planning_root = temp.path().join("planning-repo");
    std::fs::create_dir_all(&code).unwrap();
    std::fs::create_dir_all(&planning_root).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&code)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Koolade Test"]);
    git(&["config", "user.email", "koolade@example.test"]);
    std::fs::write(code.join("src.txt"), "base\n").unwrap();
    git(&["add", "src.txt"]);
    git(&["commit", "-qm", "base"]);
    git(&["switch", "-c", "branch-a"]);

    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning_root,
        StoreMode::ManagedLocal,
    );
    let original_root = planning.root.clone();
    planning
        .atomic_write(
            "planning/changes/F1/specification.md",
            b"approved plan bytes",
        )
        .unwrap();
    git(&["switch", "-c", "branch-b"]);

    assert_eq!(git_output(&code, &["branch", "--show-current"]), "branch-b");
    assert_eq!(planning.root, original_root);
    assert_eq!(
        planning
            .read("planning/changes/F1/specification.md")
            .unwrap(),
        b"approved plan bytes"
    );
}

fn git_output(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
