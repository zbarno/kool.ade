use super::*;
use std::{fs, path::Path, process::Command};

#[test]
// Production boundary: initial publication checks exact pinned Git paths before
// proposing files. A document-list test misses AGENTS.md because navigation is
// intentionally excluded from product context. Uses local Git only; low setup.
fn existing_navigation_indexes_are_never_bootstrap_overwrite_targets() {
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "--quiet", "-b", "main"]);
    git(repository.path(), &["config", "user.name", "Koolade Test"]);
    git(
        repository.path(),
        &["config", "user.email", "koolade-test@example.invalid"],
    );
    for path in ["docs/koolade/AGENTS.md", "docs/koolade/product/AGENTS.md"] {
        let file = repository.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "# Navigation\n\n- Existing human guidance.\n").unwrap();
    }
    git(repository.path(), &["add", "."]);
    git(
        repository.path(),
        &["commit", "--quiet", "-m", "existing navigation"],
    );

    let reader = crate::core::published_specs::PublishedSpecReader::new(
        repository.path(),
        "primary",
        "refs/heads/main",
    )
    .unwrap();
    let pin = reader.pin().unwrap();
    let changes = crate::core::spec_publication::initial_product_documents("Fixture");
    for path in ["docs/koolade/AGENTS.md", "docs/koolade/product/AGENTS.md"] {
        let change = changes.iter().find(|change| change.path == path).unwrap();
        let error =
            ensure_initial_destinations_are_unoccupied(&reader, &pin, std::slice::from_ref(change))
                .unwrap_err();
        assert!(error.to_string().contains(path));
    }
}

fn git(root: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
