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
    let store = koolade::artifacts::planning_store::PlanningStore::legacy_embedded(
        uuid::Uuid::nil(),
        &root,
    );
    let mut workflow = koolade::artifacts::task_docs::load_workflow(&store)?;
    let commit = koolade::core::workflow::approve_feature(&store, &mut workflow, &id)?;
    println!("Approved {id} at {commit}");
    Ok(())
}
