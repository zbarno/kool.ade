use super::*;

use crate::error::AppError;
use std::sync::atomic::AtomicUsize;

#[path = "tests/cleanup_helpers.rs"]
mod cleanup_helpers;
#[path = "tests/fixture.rs"]
mod fixture;
#[path = "tests/providers.rs"]
mod providers;
#[path = "tests/sandbox.rs"]
mod sandbox;
use cleanup_helpers::*;
use fixture::*;
use providers::*;
use sandbox::*;

#[path = "tests/auto_mode_commit.rs"]
mod auto_mode_commit;
#[path = "tests/auto_mode_conflicts.rs"]
mod auto_mode_conflicts;
#[path = "tests/cleanup.rs"]
mod cleanup;
#[path = "tests/docker_lifecycle.rs"]
mod docker_lifecycle;
#[path = "tests/initial_reconciliation.rs"]
mod initial_reconciliation;
#[path = "tests/publication_checks.rs"]
mod publication_checks;
#[path = "tests/publication_lifecycle.rs"]
mod publication_lifecycle;
#[path = "tests/recovery_corrections.rs"]
mod recovery_corrections;
#[path = "tests/resume_and_pr.rs"]
mod resume_and_pr;
#[path = "tests/task_repositories.rs"]
mod task_repositories;
#[path = "tests/verification_harness.rs"]
mod verification_harness;
#[path = "tests/worker_context.rs"]
mod worker_context;
