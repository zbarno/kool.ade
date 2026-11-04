use std::path::Path;

pub(super) fn seed_if_missing(repository_root: &Path) -> anyhow::Result<()> {
    let tracked = std::process::Command::new("git")
        .args([
            "-C",
            repository_root
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("fixture path is not UTF-8"))?,
            "cat-file",
            "-e",
            "HEAD:docs/koolade/product/overview.md",
        ])
        .status()?;
    if tracked.success() {
        return Ok(());
    }

    let product = repository_root.join("docs/koolade/product");
    std::fs::create_dir_all(&product)?;
    std::fs::write(
        product.join("overview.md"),
        "# Publication Fixture\n\nA synthetic published product for lifecycle tests.\n",
    )?;
    std::fs::write(
        product.join("index.md"),
        "# Product\n\n- [Overview](./overview.md)\n",
    )?;
    std::fs::write(
        repository_root.join("docs/koolade/AGENTS.md"),
        "# Navigation\n\n- [Product index](./product/index.md) - Published product documentation.\n",
    )?;
    std::fs::write(
        product.join("AGENTS.md"),
        "# Navigation\n\n- [Overview](./overview.md) - Product overview.\n- [index.md](./index.md) - Product document index.\n",
    )?;

    for args in [
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "add",
            "docs/koolade",
        ]
        .as_slice(),
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "Seed published lifecycle product fixture",
        ]
        .as_slice(),
    ] {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repository_root)
            .args(args)
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "could not seed synthetic published-spec docs: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
