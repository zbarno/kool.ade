//! First-launch / connect screen: pick a git repository, validate it,
//! bootstrap planning artifacts if absent, and hydrate the ~/.koolade chat.

mod connect;
pub(crate) mod github;
pub(crate) mod onboarding;
mod recent;
mod repository_picker;
pub use recent::paint_recent;
mod screen;

#[cfg(test)]
mod tests;

pub use connect::{
    PreparedConnection, attempt_connect, attempt_connect_with_store, attempt_create_project,
    attempt_create_workspace, attempt_join_workspace, attempt_open_workspace,
    prepare_open_workspace,
};
pub use github::{
    GithubTarget, clone_destination, parse_github_url, perform_clone, perform_clone_at,
};
pub use screen::paint;
