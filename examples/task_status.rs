//! Read-only audit of board task identities and implementation status.
fn main() -> anyhow::Result<()> {
    let repo = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()))
        .canonicalize()?;
    let workflow = koolade::artifacts::task_docs::load_workflow(&repo)?;
    let docs = koolade::artifacts::task_docs::load_board(&repo, &workflow);
    let states = koolade::core::implementation::load_board_states(&repo);
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for doc in docs.iter().filter(|doc| !doc.path.ends_with("/README.md")) {
        // Exercise the same read-only ticket/target validation as Resume.
        koolade::core::implementation::target_repository(&repo, &doc.path)?;
        let record = states.get(&doc.path);
        if let Some(board) = record {
            let loaded =
                koolade::core::implementation::load(&repo, &doc.path).ok_or_else(|| {
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
                    loaded.worktree.join(".git").is_file(),
                    "Preserved worktree is missing: {}",
                    loaded.worktree.display()
                );
                koolade::core::implementation::completed_dependency_context(
                    &repo, &doc.path, &doc.text,
                )?;
                println!(
                    "Resume lookup and merged dependencies verified; preserved worktree: {}",
                    loaded.worktree.display()
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
