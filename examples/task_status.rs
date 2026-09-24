//! Read-only audit of board task identities and implementation status.
fn main() -> anyhow::Result<()> {
    let repo = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()))
        .canonicalize()?;
    let workflow = packet::artifacts::task_docs::load_workflow(&repo)?;
    let docs = packet::artifacts::task_docs::load_board(&repo, &workflow);
    let states = packet::core::implementation::load_board_states(&repo);
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for doc in docs.iter().filter(|doc| !doc.path.ends_with("/README.md")) {
        let record = states.get(&doc.path);
        let status = record.map(|state| state.status.as_str()).unwrap_or("To do");
        *counts.entry(status.into()).or_default() += 1;
        println!("{status}\t{}", doc.path);
        if let Some(state) = record.filter(|state| state.ticket != doc.path) {
            println!("  Preserved evidence identity: {}", state.ticket);
        }
    }
    println!("Totals: {counts:?}");
    Ok(())
}
