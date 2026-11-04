use super::*;
use crate::artifacts::IMPORTS_DIR;
use std::path::{Path, PathBuf};

fn sandbox(prefix: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("koolade_imp_{prefix}_{}", std::process::id()));
    let outside =
        std::env::temp_dir().join(format!("koolade_outside_{prefix}_{}", std::process::id()));
    for d in [&root, &outside] {
        let _ = std::fs::remove_dir_all(d);
        std::fs::create_dir_all(d).unwrap();
    }
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    (root, outside)
}

fn local_store(root: &Path) -> PlanningStore {
    PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join("private-app-data"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    )
}

#[test]
fn textual_source_gets_companion_and_collision_numbering() {
    let (repo, out) = sandbox("txt");
    let store = local_store(&out);
    let src = out.join("notes.txt");
    std::fs::write(&src, "hello\nworld\n").unwrap();
    let expected = store.revision().unwrap();
    let (r1, expected) = import_into_store(&store, &src, &expected).unwrap();
    let (r2, _) = import_into_store(&store, &src, &expected).unwrap();
    assert_eq!(r1.stored_name, "notes.txt");
    assert_eq!(r1.companion.as_deref(), Some("notes-txt.md"));
    assert!(r1.note.is_none());
    assert_eq!(r2.stored_name, "notes-1.txt");
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn markdown_sources_do_not_duplicate_themselves() {
    let (repo, out) = sandbox("md");
    let store = local_store(&out);
    let src = out.join("design.md");
    std::fs::write(&src, "# hi\n").unwrap();
    let (r, _) = import_into_store(&store, &src, &store.revision().unwrap()).unwrap();
    assert_eq!(r.stored_name, "design.md");
    assert!(r.companion.is_none());
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn managed_imports_write_to_the_selected_store_and_return_its_revision() {
    let (repo, out) = sandbox("managed");
    let store = local_store(&out);
    let source = out.join("notes.txt");
    std::fs::write(&source, "managed evidence").unwrap();
    let expected = store.revision().unwrap();

    let (doc, revision) = import_into_store(&store, &source, &expected).unwrap();

    assert_eq!(doc.stored_name, "notes.txt");
    assert_eq!(
        store.read("planning/imports/notes.txt").unwrap(),
        b"managed evidence"
    );
    assert_eq!(
        store.read("planning/imports/notes-txt.md").unwrap(),
        b"managed evidence"
    );
    assert_eq!(revision, store.revision().unwrap());
    assert!(!repo.join(IMPORTS_DIR).exists());
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn refuses_self_copy_from_a_separate_managed_planning_store() {
    let (repo, out) = sandbox("managed_self");
    let store = local_store(&out);
    let imports = store.root.join("planning/imports");
    std::fs::create_dir_all(&imports).unwrap();
    let source = imports.join("existing.txt");
    std::fs::write(&source, "already in the planning store").unwrap();
    let expected = store.revision().unwrap();

    let error = import_into_store(&store, &source, &expected).unwrap_err();

    assert!(error.to_string().contains("inside the planning store"));
    assert!(!imports.join("existing-1.txt").exists());
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn binary_sources_are_flagged_but_kept() {
    let (repo, out) = sandbox("bin");
    let store = local_store(&out);
    let src = out.join("diagram.bin");
    std::fs::write(&src, vec![0xFF, 0xD8, 0x00]).unwrap();
    let (r, _) = import_into_store(&store, &src, &store.revision().unwrap()).unwrap();
    assert_eq!(r.stored_name, "diagram.bin");
    assert!(r.note.is_some());
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn refuses_self_copy_from_the_local_store() {
    let (repo, out) = sandbox("self");
    let store = local_store(&out);
    let imports = store.root.join("planning/imports");
    std::fs::create_dir_all(&imports).unwrap();
    let inside = imports.join("a.txt");
    std::fs::write(&inside, "x").unwrap();
    assert!(import_into_store(&store, &inside, &store.revision().unwrap()).is_err());
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn list_reports_sorted_entries() {
    let (repo, out) = sandbox("list");
    let store = local_store(&out);
    let imports = store.root.join("planning/imports");
    std::fs::create_dir_all(&imports).unwrap();
    std::fs::write(imports.join("b.md"), "bb").unwrap();
    std::fs::write(imports.join("a.md"), "a").unwrap();
    let v = list_imports_in_store(&store).unwrap();
    assert_eq!(
        v.iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
        vec!["a.md", "b.md"]
    );
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&out);
}

#[cfg(unix)]
#[test]
fn list_rejects_symlinked_import_parent_and_entries() {
    use std::os::unix::fs::symlink;

    let (repo, outside) = sandbox("list_symlink");
    let store = local_store(&outside);
    let imports = store.root.join("planning/imports");
    std::fs::create_dir_all(&store.root).unwrap();
    std::fs::create_dir_all(&imports).unwrap();
    std::fs::write(outside.join("secret.txt"), "outside data").unwrap();
    symlink(outside.join("secret.txt"), imports.join("linked.txt")).unwrap();

    assert!(matches!(
        list_imports_in_store(&store),
        Err(StoreError::InvalidPath(_))
    ));
    std::fs::remove_file(imports.join("linked.txt")).unwrap();
    std::fs::remove_dir_all(store.root.join("planning")).unwrap();
    symlink(&repo, store.root.join("planning")).unwrap();
    assert!(matches!(
        list_imports_in_store(&store),
        Err(StoreError::InvalidPath(_))
    ));

    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&outside);
}
