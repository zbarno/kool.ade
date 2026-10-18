//! Persisted lifecycle acceptance fixtures for #157–#166 and #172.
//! The completed path exercises the production QA orchestrator and lifecycle
//! stores with deterministic adapters; rootless candidate-image workflows
//! separately prove browser and desktop interaction.

#[path = "lifecycle_acceptance/blocked_path.rs"]
mod blocked_path;
#[path = "lifecycle_acceptance/completed_path.rs"]
mod completed_path;
#[path = "lifecycle_acceptance/headless_applicability.rs"]
mod headless_applicability;
#[path = "lifecycle_acceptance/plan.rs"]
mod plan;
#[path = "lifecycle_acceptance/support.rs"]
mod support;
#[path = "lifecycle_acceptance/task_review.rs"]
mod task_review;
