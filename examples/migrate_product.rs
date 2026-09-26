//! One-time, idempotent migration of a connected Packet project.
//! Usage: cargo run --offline --example migrate_product -- /path/to/repository
use std::path::PathBuf;
fn main() -> anyhow::Result<()> {
    let root =
        PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into())).canonicalize()?;
    anyhow::ensure!(
        packet::core::gitops::is_work_tree(&root),
        "Not a git worktree"
    );
    let paths = packet::artifacts::migration::run(&root)?;
    if paths.is_empty() {
        println!("Packet artifacts already use the current schema; no files changed.");
        return Ok(());
    }
    println!(
        "Migrated and checkpointed {} Packet artifact paths.",
        paths.len()
    );
    Ok(())
}
