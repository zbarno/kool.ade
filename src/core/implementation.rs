//! Resumable ticket implementation. Task repositories and runtime records are
//! kept independently from planning state; only verified results proceed to a PR.
mod activity;
mod board_states;
mod checks;
mod checks_gate;
pub mod cleanup;
mod code_review;
mod controller;
mod execution;
mod initial_reconciliation;
mod integration;
mod lifecycle;
mod pr_refresh;
mod publication;
mod queries;
mod recovery;
pub(crate) mod report;
mod repository_cache;
mod runner;
mod state;
mod state_paths;
pub mod status;
mod task;
mod task_repository;
mod telemetry;
mod verification;
pub use activity::{finalize_terminal_activity_if_stale, load_activity, save_activity};
pub use board_states::{
    BOARD_COLUMNS, board_column, load_board_states, load_board_states_with_store,
};
pub use controller::Controller;
pub(crate) use controller::ProjectStartRequest;
pub(crate) use execution::mark_resume_started;
pub(crate) use execution::record_failed_attempt;
pub use execution::run;
use execution::run_with_planning_store_options;
#[cfg(test)]
use execution::run_with_project_options;
pub use pr_refresh::PrRefresh;
#[cfg(test)]
use pr_refresh::refresh_pr;
#[cfg(test)]
pub(crate) use pr_refresh::refresh_pr_with_store_for_test;
use publication::pr_body;
#[cfg(test)]
use publication::remote_repository;
use queries::{history_preflight_context, resume_failure_context};
pub use queries::{load, load_all, load_with_store};
pub use recovery::latest_external_blocker;
pub(crate) use report::{BlockerDisposition, Report, ReportStatus, parse_report};
use report::{external_blocker, external_blocker_detail, validate_report};
use runner::{Runner, append_tail};
pub(crate) use state::save;
pub(crate) use state::{decode_state_bytes, read_state_file, serialize_state};
use state_paths::{common, key};
pub(crate) use state_paths::{key_for_ticket, state_dir, state_dir_for_task};
pub(crate) use task::permits_evidence_only_completion;
#[cfg(test)]
use task::read_ticket;
#[cfg(test)]
use task::scoped_product_context;
pub(crate) use task::task_repository_id;
pub(crate) use task::task_repository_id_with_store;
pub use task::{completed_dependency_context, target_repository};
pub(crate) use task::{
    completed_dependency_context_with_store, scoped_product_context_with_store,
    target_repository_with_store,
};
use task::{read_ticket_and_identity, specification_matches_task, ticket_identity, title};
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
#[path = "implementation/metadata_tests.rs"]
mod metadata_tests;
pub use status::{
    Failure, FailureKind, ImplementationStatus, IndependentCheck, IndependentCheckStatus,
    PublicationStatus, PullRequestDetails, PullRequestState, RecoveryDisposition,
};

use crate::harness::{AiHarness, LiveProgress, PlanningRequest};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskRepositoryKind {
    Clone,
    #[default]
    LegacyWorktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    pub ticket: String,
    /// Stable task identity; `ticket` remains the current path hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_uid: Option<String>,
    pub ticket_text: String,
    #[serde(default)]
    pub approved_specification: Option<String>,
    #[serde(default)]
    pub approved_product_context: Option<String>,
    #[serde(default)]
    pub completed_dependency_context: Option<String>,
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_branch: Option<String>,
    /// The source ref and immutable commit selected when this task was dispatched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
    /// Stable project-manifest identity and its non-secret repository identity hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Planning-store identity that owns this isolated task clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_identity: Option<String>,
    /// Configured push repository identity captured before the worker starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_cache: Option<PathBuf>,
    /// Original task key used for private clone/worktree allocations and refs.
    /// It stays fixed if the ticket path changes while work is in progress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_repository_allocation_key: Option<String>,
    pub base: String,
    pub base_commit: String,
    #[serde(rename = "task_repository", alias = "worktree")]
    pub task_repository: PathBuf,
    #[serde(default)]
    pub task_repository_kind: TaskRepositoryKind,
    #[serde(default)]
    pub task_repository_ready: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_repositories: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub task_repository_commits: BTreeMap<String, String>,
    pub status: ImplementationStatus,
    pub detail: String,
    pub pr_url: Option<String>,
    pub verified_head: Option<String>,
    #[serde(default)]
    pub auto_merge: bool,
    #[serde(default)]
    pub merged_commit: Option<String>,
    #[serde(default)]
    pub pr_state: Option<PullRequestState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_details: Option<PullRequestDetails>,
    #[serde(default)]
    pub pr_checked_at: Option<String>,
    #[serde(default)]
    pub pr_check_attempted_at: Option<String>,
    #[serde(default)]
    pub pr_check_error: Option<String>,
    #[serde(default)]
    pub independent_check: Option<IndependentCheck>,
    #[serde(default)]
    pub cleanup: cleanup::Cleanup,
}
pub enum Event {
    Progress(Box<LiveProgress>),
    Done(Box<Result<Implementation, Failure>>),
    ClaimBlocked(Box<crate::core::task_claim::ClaimError>),
    ClaimWarning(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublicationMode {
    HoldForReview,
    CreatePullRequest,
    AutoPublish,
}

pub(crate) struct StartPolicy {
    pub publication_mode: PublicationMode,
    pub require_independent_checks: bool,
}

struct RunOptions<'a> {
    harness: &'a dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
    gh: &'a str,
    publication_mode: PublicationMode,
    require_independent_checks: bool,
    user_context: Option<&'a str>,
    auto_publish_gate: Option<Arc<AtomicBool>>,
    claim_lease: Option<crate::core::task_claim::ClaimLeaseHandle>,
}

struct ExecutionPolicy<'a> {
    user_context: Option<&'a str>,
    publication_mode: PublicationMode,
    require_independent_checks: bool,
    auto_publish_gate: Option<&'a AtomicBool>,
    claim_lease: Option<&'a crate::core::task_claim::ClaimLeaseHandle>,
    /// Per-workspace accrual scope (F7): `Some` when identifiers resolved.
    accrual: Option<crate::core::time_accrual::AgentSpan>,
}

#[cfg(test)]
#[path = "implementation/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "implementation/activity_persist_trims.rs"]
mod activity_persist_trims;
