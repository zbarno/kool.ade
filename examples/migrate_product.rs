//! One-time, idempotent migration of a connected Koolade project.
//! Usage: cargo run --offline --example migrate_product -- /path/to/repository
use std::path::PathBuf;
fn main() -> anyhow::Result<()> {
    let root =
        PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into())).canonicalize()?;
    anyhow::ensure!(
        koolade::core::gitops::is_work_tree(&root),
        "Not a git worktree"
    );
    let paths = koolade::artifacts::migration::run(&root)?;
    if paths.is_empty() {
        println!("Koolade artifacts already use the current schema; no files changed.");
        return Ok(());
    }
    println!(
        "Migrated and checkpointed {} Koolade artifact paths.",
        paths.len()
    );
    Ok(())
}
