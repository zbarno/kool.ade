use crate::{app::session::Project, core::lifecycle::LifecycleStore};
use prerequisites::{event_uid, unavailable_reason};

mod advance;
mod prerequisites;
pub(crate) mod recipe_queue;
mod reports;
mod request;
mod worker;

/// Reconciles durable QA requests on every project tick. Missing trusted
/// runtime assets become persisted blockers; this path never invents test
/// commands, reviewer provenance, or a passing result.
pub(super) use advance::advance_runtime_qa;
