//! One-time, idempotent migration of a connected planning root.
//! Usage: cargo run --offline --example migrate_product -- /path/to/repository
use std::path::PathBuf;
fn main() -> anyhow::Result<()> {
    let root =
        PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into())).canonicalize()?;
    anyhow::ensure!(
        packet::core::gitops::is_work_tree(&root),
        "Not a git worktree"
    );
    let legacy =
        std::fs::read_to_string(root.join(packet::artifacts::SPEC_FILE)).unwrap_or_default();
    let paths = packet::artifacts::product_docs::migrate(&root, &legacy)?;
    if paths.is_empty() {
        println!("Product modules already exist; no files changed.");
        return Ok(());
    }
    packet::core::gitops::commit(&root, "planner: migrate product specification", &paths)
        .map_err(|e| anyhow::anyhow!("Migration files are preserved; checkpoint failed: {e}"))?;
    println!(
        "Migrated {} tracked paths into planning/product/",
        paths.len()
    );
    Ok(())
}
