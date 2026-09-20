//! Runtime persistence OUTSIDE the git repository.
//!
//! Per product decision: chat history persists per-operator under
//! `~/.packet/` (overridable with `$PACKET_HOME`), keyed by repository
//! slug. Git remains the sole store for *shared* planning artifacts.
//!
//! * `home.rs` — `~/.packet` location math and project slugs
//! * `chat_store.rs` — JSONL chat history per project

pub mod chat_store;
pub mod task_chats;
pub mod archived_tasks;

mod home;

pub use home::{fnv1a64, known_projects_path, project_dir, project_slug, state_root};
