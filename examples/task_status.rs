//! Read-only audit of board task identities and implementation status.
//! Usage: cargo run --example task_status -- PLANNING_REPOSITORY CODE_REPOSITORY
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let planning_repo = std::path::PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("planning repository required"))?,
    )
    .canonicalize()?;
    let code_repo = std::path::PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("code repository required"))?,
    )
    .canonicalize()?;
    anyhow::ensure!(args.next().is_none(), "unexpected argument");
    let store = koolade::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::nil(),
        &planning_repo,
        koolade::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let workflow = koolade::artifacts::task_docs::load_workflow(&store)?;
    let docs = koolade::artifacts::task_docs::load_board(&store, &workflow);
    let states = koolade::core::implementation::load_board_states_with_store(&code_repo, &store);
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for doc in docs.iter().filter(|doc| !doc.path.ends_with("/README.md")) {
        // Exercise the same read-only ticket/target validation as Resume.
        koolade::core::implementation::target_repository_with_store(&store, &code_repo, &doc.path)?;
        let record = states.get(&doc.path);
        if let Some(board) = record {
            let loaded =
                koolade::core::implementation::load(&code_repo, &doc.path).ok_or_else(|| {
                    anyhow::anyhow!("Board shows a record but Resume cannot load {}", doc.path)
                })?;
            anyhow::ensure!(
                loaded == *board,
                "Board and Resume disagree for {}",
                doc.path
            );
            anyhow::ensure!(
                loaded.ticket_text == doc.text,
                "Frozen ticket changed: {}",
                doc.path
            );
            if loaded.status != koolade::core::implementation::ImplementationStatus::Completed {
                anyhow::ensure!(
                    loaded.task_repository.join(".git").exists(),
                    "Preserved task repository is missing: {}",
                    loaded.task_repository.display()
                );
                koolade::core::implementation::completed_dependency_context_with_store(
                    &store, &code_repo, &doc.path, &doc.text,
                )?;
                println!(
                    "Resume lookup and merged dependencies verified; preserved task repository: {}",
                    loaded.task_repository.display()
                );
            }
        }
        let status = record.map(|state| state.status.label()).unwrap_or("To do");
        *counts.entry(status.into()).or_default() += 1;
        println!("{status}\t{}", doc.path);
        if let Some(state) = record.filter(|state| state.ticket != doc.path) {
            println!("  Preserved evidence identity: {}", state.ticket);
        }
    }
    println!("Totals: {counts:?}");
    Ok(())
}
