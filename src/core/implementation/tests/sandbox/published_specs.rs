use std::path::Path;

pub(super) fn seed_if_missing(repository: &Path) -> anyhow::Result<()> {
    if repository
        .join("docs/koolade/product/overview.md")
        .is_file()
    {
        return Ok(());
    }
    let product = repository.join("docs/koolade/product");
    std::fs::create_dir_all(&product)?;
    std::fs::write(
        product.join("overview.md"),
        "# Product Fixture\n\nA synthetic published product for implementation tests.\n",
    )?;
    std::fs::write(
        product.join("index.md"),
        "# Product\n\n- [Overview](./overview.md)\n",
    )?;
    std::fs::write(
        repository.join("docs/koolade/AGENTS.md"),
        "# Navigation\n\n- [Product index](./product/index.md) - Published product docs.\n",
    )?;
    std::fs::write(
        product.join("AGENTS.md"),
        "# Navigation\n\n- [Overview](./overview.md) - Product overview.\n- [index.md](./index.md) - Product index.\n",
    )?;
    for arguments in [
        ["add", "docs/koolade"].as_slice(),
        ["commit", "-qm", "Seed published implementation fixture"].as_slice(),
    ] {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repository)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(arguments)
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "could not seed implementation product docs: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
