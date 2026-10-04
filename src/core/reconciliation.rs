//! Post-merge feature reconciliation. Only merged task batches qualify; the
//! model may replace affected product modules or raise an explicit review item.
mod candidate;
mod controller;
mod evidence;
mod response;
mod run;
pub use candidate::{Candidate, candidate};
pub use controller::Controller;
use evidence::implementation_evidence;
use response::{prompt, validate_response};
#[cfg(test)]
use run::run_with_settle_window;
pub use run::{DEFER_PREFIX, run};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::AtomicBool,
        mpsc::{self, Receiver},
    },
};

use crate::{
    core::{
        apply, gitops, implementation::ImplementationStatus, state::PlannerState, validation,
        workflow,
    },
    domain::Authority,
    harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest, TurnEnvelope},
};

#[cfg(test)]
#[path = "reconciliation/tests.rs"]
mod tests;
