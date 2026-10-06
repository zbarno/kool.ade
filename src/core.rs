//! Planning engine (SPECIFICATION.md §10–§12, §16–§17, §20).
//!
//! Submodule map:
//! * `ids` — open-item identifier handling (CLR-NNN)
//! * `routing` — per-user question eligibility (the crux behavior, §11)
//! * `ownership` — auto-creation of Ownership items (§8)
//! * `state` — the loaded, in-memory project state
//! * `gitops` — minimal git behavior (§20)
//! * `repo_overview` — cheap context the harness should still verify
//! * `context_build` / `prompt` — what each planning turn ships to pi
//! * `validation` / `apply` — verify the structured response, then mutate
//! * `turn` — the threaded turn pipeline driving the UI

pub mod apply;
pub mod attention;
pub mod board_actions;
pub mod context_build;
pub mod context_retrieval;
pub mod contract_snapshot;
pub mod gitops;
pub mod ids;
pub mod investigation;
pub mod ownership;
pub mod prompt;
pub mod reconciliation;
pub mod repo_overview;
pub mod routing;
pub mod state;
pub mod turn;
pub mod validation;
pub mod writer_gate;

pub mod workflow;

pub mod task_generation;

pub mod implementation;
pub mod planning_work;

pub mod implementation_queue;

pub mod specification;

pub mod project_repos;
pub mod task_claim;
pub mod task_conversation;
pub mod time_accrual;
pub mod time_totals;
