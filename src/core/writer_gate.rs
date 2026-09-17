//! Process-wide planning-writer gate.
//!
//! Packet connects ONE repository per process at a time, so a single gate is
//! sufficient. Every subsystem that writes planning artifacts OR checkpoints
//! the planning repository — interview/task turns, reconciliation,
//! investigations, board decisions, feature approval, imports, MCP and
//! settings edits, and a worker's best-effort publication fast-forward —
//! must hold this gate for its ENTIRE file-write-plus-git-commit
//! transaction. Holding the gate only around the git call would let a second
//! writer rewrite artifacts underneath the first mid-commit.
//!
//! Serialization alone does not prevent lost updates: writers snapshot state
//! at start and would happily commit stale bytes over a rival's newer commit.
//! Each writer therefore ALSO runs its drift guard (see
//! [`crate::core::state::PlannerState`] / `drift_report`) inside the gate and
//! refuses to apply a stale snapshot instead of clobbering.
//!
//! Deliberate exemptions:
//! - Task-story STREAMING (`task_generation` publish progress) writes
//!   uncommitted working files across many model rounds; it never touches the
//!   git index and its final commit goes through the gated apply path, so it
//!   opts out of holding the gate across multi-hour generations.
//! - Cross-process actors (humans, editors, a second Packet instance) cannot
//!   hold this in-process gate; per-writer drift baselines plus explicit-path
//!   commits are the defense against them.

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
