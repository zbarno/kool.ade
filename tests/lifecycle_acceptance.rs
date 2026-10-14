//! Persisted lifecycle acceptance fixtures for #157–#165 and #172.
//! These exercise production record reducers and stores; they do not launch a
//! real browser or desktop QA driver.

#[path = "lifecycle_acceptance/blocked_path.rs"]
mod blocked_path;
#[path = "lifecycle_acceptance/completed_path.rs"]
mod completed_path;
#[path = "lifecycle_acceptance/plan.rs"]
mod plan;
#[path = "lifecycle_acceptance/support.rs"]
mod support;
