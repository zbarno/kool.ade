//! Private application persistence OUTSIDE source repositories and task clones.
//!
//! TaskIntents, tasks, conversations, approvals, runtime evidence, and other
//! operational state live under `~/.koolade-packet/` (overridable with
//! `$KOOLADE_HOME`), keyed by stable workspace identity. The primary code
//! repository is authoritative only for published specifications and source.
//!
//! * `home.rs` — `~/.koolade` location math and project slugs
//! * `chat_store.rs` — JSONL chat history per project
//! * `persona.rs` — operator-level persona document at the state root
//! * `telemetry/` — F11 invocation records: versioned JSONL, append store

pub mod archived_tasks;
pub mod bootstrap_conversations;
pub mod cancelled_work;
pub mod chat_store;
pub mod dependency_authorization;
pub mod discovery_log;
pub mod execution_environment_settings;
pub mod execution_records;
pub mod experimental_features;
pub(crate) mod feature_approvals;
pub mod harness_settings;
// These schemas are used by Linux rootless Docker paths and remain declared
// for shared call sites that return early on other operating systems.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) mod linux_rootless_docker_selection;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) mod linux_rootless_smoke_receipt;
pub mod persona;
pub(crate) mod project_guidance;
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

pub(crate) use home::is_inside_git_worktree;
pub use home::{fnv1a64, known_projects_path, project_dir, project_slug, state_root};
