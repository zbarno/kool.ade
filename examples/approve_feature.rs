//! Record a human-approved feature contract in the planning-root workflow.
//! Usage: cargo run --offline --example approve_feature -- REPOSITORY CHG-001
use std::path::PathBuf;
fn main() -> anyhow::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let root = PathBuf::from(
        arguments
            .next()
            .ok_or_else(|| anyhow::anyhow!("repository required"))?,
    )
    .canonicalize()?;
    let id = arguments
        .next()
        .ok_or_else(|| anyhow::anyhow!("feature ID required"))?;
    anyhow::ensure!(arguments.next().is_none(), "unexpected argument");
    let mut workflow = packet::artifacts::task_docs::load_workflow(&root)?;
    let commit = packet::core::workflow::approve_feature(&root, &mut workflow, &id)?;
    println!("Approved {id} at {commit}");
    Ok(())
}
