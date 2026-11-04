//! Process-wide gate for local operational-state writers.
//!
//! Koolade connects one project per process at a time, so a single gate is
//! sufficient. Writers hold it for the complete local transaction, including
//! interview/task turns, reconciliation, investigations, board decisions,
//! feature approval, imports, MCP, settings, and worker publication state.
//!
//! Serialization alone does not prevent lost updates: writers snapshot state
//! at start and could overwrite a rival's newer local revision. Each writer
//! also checks its drift baseline inside the gate and refuses stale writes.
//!
//! Cross-process actors use durable per-store locks and record revisions; the
//! in-process gate covers coordinated writers in this application instance.

use std::sync::{Mutex, MutexGuard};

static PLANNER_WRITER: Mutex<()> = Mutex::new(());

/// Held for one write+checkpoint transaction; releasing ends the section.
#[derive(Debug)]
pub struct Guard(
    // Field carries no data; its Drop is the entire contract.
    #[allow(dead_code)] MutexGuard<'static, ()>,
);

/// Enter the writer section. Blocks until every other writer has committed
/// and released. Writers are expected to be brief (milliseconds to a few
/// seconds); the UI thread may call this too — the longest critical section
/// it can meet is one finishing turn's checkpoint.
pub fn acquire() -> Guard {
    Guard(PLANNER_WRITER.lock().expect("planner writer gate poisoned"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn serializes_concurrent_writers() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..25 {
                    let _held = acquire();
                    let cur = counter.load(Ordering::SeqCst);
                    counter.store(cur + 1, Ordering::SeqCst);
                    // While WE hold the gate no other writer may observe or
                    // move the counter between our store and this re-read.
                    let again = counter.load(Ordering::SeqCst);
                    assert_eq!(again, cur + 1, "gate leaked concurrency");
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(counter.load(Ordering::SeqCst), 100);
    }
}
