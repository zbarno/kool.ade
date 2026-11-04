use std::{path::Path, process::Command};

pub(crate) fn project_id_for_code_root(repo: &Path) -> uuid::Uuid {
    let canonical = std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        canonical.to_string_lossy().as_bytes(),
    )
}

/// Test boundary: state-loading tests seed a minimal pinned product document
/// and feature navigation index in a temporary code repository. This keeps
/// fixtures representative of the production Git reader without restoring
/// bootstrap or importer paths.
pub(crate) fn seed_published_product_document(repo: &Path) -> anyhow::Result<()> {
    let root = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    let root = if root.status.success() {
        std::path::PathBuf::from(String::from_utf8(root.stdout)?.trim())
    } else {
        std::fs::create_dir_all(repo)?;
        let initialized = Command::new("git")
            .args(["-C"])
            .arg(repo)
            .args(["init", "--quiet", "-b", "main"])
            .status()?;
        anyhow::ensure!(
            initialized.success(),
            "could not initialize test code repository"
        );
        repo.to_path_buf()
    };
    let overview = root.join("docs/koolade/product/overview.md");
    let product_index = root.join("docs/koolade/product/index.md");
    let koolade_navigation = root.join("docs/koolade/AGENTS.md");
    let product_navigation = root.join("docs/koolade/product/AGENTS.md");
    let feature_index = root.join("docs/koolade/features/AGENTS.md");
    if !overview.exists() {
        std::fs::create_dir_all(overview.parent().expect("product document parent"))?;
        std::fs::write(
            &overview,
            "# Test Product Overview — Living Technical Specification\n\n**Version:** 1.0. **Status:** Accepted fixture.\n\n## Overview\n\nTest product context.\n\n## Users and Outcomes\n\nSynthetic users can complete the tested workflow.\n\n## Current Capabilities\n\nOnly test capabilities are described.\n\n## Architecture and Constraints\n\nTests use temporary repositories and local storage.\n\n## Decisions\n\n| ID | Decision | Basis | Status |\n| --- | --- | --- | --- |\n\n## Quality and Acceptance\n\nAssertions verify the requested test boundary.\n",
        )?;
    }
    if !product_index.exists() {
        std::fs::create_dir_all(product_index.parent().expect("product index parent"))?;
        std::fs::write(
            &product_index,
            "# Product documents\n\n- [Overview](./overview.md)\n",
        )?;
    }
    if !koolade_navigation.exists() {
        std::fs::create_dir_all(koolade_navigation.parent().expect("Koolade docs parent"))?;
        std::fs::write(
            &koolade_navigation,
            "# Navigation\n\n- [Product index](./product/index.md) - Published product documents.\n- [Product navigation](./product/AGENTS.md) - Product document index.\n- [Features navigation](./features/AGENTS.md) - Published feature index.\n",
        )?;
    }
    if !product_navigation.exists() {
        std::fs::write(
            &product_navigation,
            "# Navigation\n\n- [Product index](./index.md) - Product document index.\n- [Overview](./overview.md) - Published product overview.\n",
        )?;
    }
    if !feature_index.exists() {
        std::fs::create_dir_all(feature_index.parent().expect("feature index parent"))?;
        std::fs::write(&feature_index, "# Navigation\n")?;
    }
    let add = Command::new("git")
        .args(["-C"])
        .arg(&root)
        .args([
            "add",
            "--",
            "docs/koolade/product/overview.md",
            "docs/koolade/product/index.md",
            "docs/koolade/AGENTS.md",
            "docs/koolade/product/AGENTS.md",
            "docs/koolade/features/AGENTS.md",
        ])
        .status()?;
    anyhow::ensure!(add.success(), "could not stage published test context");
    let staged = Command::new("git")
        .args(["-C"])
        .arg(&root)
        .args(["diff", "--cached", "--quiet"])
        .status()?;
    if staged.success() {
        return Ok(());
    }
    let commit = Command::new("git")
        .args(["-C"])
        .arg(&root)
        .args([
            "-c",
            "user.name=Kool.ad/e Test",
            "-c",
            "user.email=koolade@test.local",
            "commit",
            "-m",
            "Seed synthetic published test documentation",
        ])
        .status()?;
    anyhow::ensure!(commit.success(), "could not commit published test context");
    let branch = Command::new("git")
        .args(["-C"])
        .arg(&root)
        .args(["branch", "-M", "main"])
        .status()?;
    anyhow::ensure!(
        branch.success(),
        "could not set test default branch to main"
    );
    Ok(())
}
