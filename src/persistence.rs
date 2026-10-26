//! Runtime persistence OUTSIDE the git repository.
//!
//! Per product decision: chat history persists per-operator under
//! `~/.koolade-packet/` (overridable with `$KOOLADE_HOME`), keyed by repository
//! slug. Git remains the sole store for *shared* planning artifacts.
//!
//! * `home.rs` — `~/.koolade` location math and project slugs
//! * `chat_store.rs` — JSONL chat history per project
//! * `persona.rs` — operator-level persona document at the state root
//! * `telemetry/` — F11 invocation records: versioned JSONL, append store

pub mod archived_tasks;
pub mod cancelled_work;
pub mod chat_store;
pub mod dependency_authorization;
pub mod discovery_log;
pub mod execution_environment_settings;
pub mod execution_records;
pub(crate) mod feature_approvals;
pub mod harness_settings;
pub(crate) mod linux_rootless_docker_selection;
pub(crate) mod linux_rootless_smoke_receipt;
pub mod persona;
pub mod project_harness_routes;
pub mod remote_execution_telemetry;
pub(crate) mod remote_private_file_executions;
pub(crate) mod remote_private_file_grants;
pub mod remote_requests;
pub mod remote_workers;
pub(crate) mod runtime_target_approvals;
pub mod setup_tasks;
pub mod task_chats;
pub mod telemetry;

mod home;

pub use home::{fnv1a64, known_projects_path, project_dir, project_slug, state_root};
