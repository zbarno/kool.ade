//! Run the same conservative maintenance worker as the desktop, without opening a UI.
//! Usage: cargo run --example cleanup_completed -- /absolute/planning/repository
use koolade::core::implementation::{ImplementationStatus, PrRefresh, load_all};
use std::{path::PathBuf, time::Duration};
fn main() -> anyhow::Result<()> {
    let repo = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Expected repository path"))?,
    )
    .canonicalize()?;
    let mut states = load_all(&repo);
    states.sort_by_key(|state| state.cleanup.attempted_at.clone());
    let mut pending = false;
    // One ticket per bounded worker prevents large backlogs starving later tickets.
    for state in states.into_iter().filter(|state| {
        state.status == ImplementationStatus::Completed && state.cleanup.completed_at.is_none()
    }) {
        let worker = PrRefresh::start(repo.clone(), vec![state.ticket.clone()]);
        loop {
            if let Some(errors) = worker.poll() {
                for (ticket, error) in errors {
                    eprintln!("{ticket}: {error}");
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let current = koolade::core::implementation::load(&repo, &state.ticket)
            .ok_or_else(|| anyhow::anyhow!("Task state disappeared: {}", state.ticket))?;
        pending |= current.cleanup.completed_at.is_none();
        println!(
            "{}: {}",
            state.ticket,
            current
                .cleanup
                .error
                .as_deref()
                .unwrap_or(if current.cleanup.completed_at.is_some() {
                    "cleanup complete; evidence retained"
                } else {
                    "pending; task may be active"
                })
        );
    }
    anyhow::ensure!(
        !pending,
        "Some cleanup remains pending; see per-task diagnostics above"
    );
    Ok(())
}
