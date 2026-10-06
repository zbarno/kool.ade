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
pub mod chat_store;
pub mod persona;
pub mod task_chats;
pub mod telemetry;

mod home;

pub use home::{fnv1a64, known_projects_path, project_dir, project_slug, state_root};
