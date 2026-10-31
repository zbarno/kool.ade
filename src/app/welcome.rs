//! First-launch / connect screen: pick a git repository, validate it,
//! bootstrap planning artifacts if absent, and hydrate the ~/.koolade chat.

mod connect;
pub(crate) mod github;
mod github_pair;
pub(crate) mod onboarding;
mod recent;
pub use recent::paint_recent;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use connect::attempt_connect_with_store;
pub use connect::{
    PreparedConnection, attempt_connect, attempt_create_project, attempt_open_workspace,
    prepare_open_workspace,
};
pub use github::{
    GithubTarget, clone_destination, parse_github_url, perform_clone, perform_clone_at,
};
pub(crate) use github_pair::perform_clone_and_pair;
