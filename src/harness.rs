//! The external-AI-harness boundary (SPECIFICATION.md §14–§15).
//!
//! Named-file module layout:
//! * `harness.rs`      — trait, request/outcome/envelope types
//! * `pi_harness.rs`   — the Pi CLI implementation (only MVP backend)
//! * `pi_proc.rs`      — child-process supervision primitives
//! * `pi_events.rs`    — NDJSON event-stream folding
//! * `pi_extract.rs`   — JSON-block extraction from final prose

pub mod pi_events;
pub mod live_preview;
pub mod pi_extract;
pub mod pi_harness;
pub mod pi_proc;

pub use harness::{AiHarness, HarnessOutcome, LiveProgress, PlanningRequest, TurnEnvelope, TurnItem, TurnItemUpdate};
pub use pi_harness::PiHarness;

mod harness;
