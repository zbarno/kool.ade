//! First-launch / connect screen: pick a git repository, validate it,
//! bootstrap planning artifacts if absent, and hydrate the ~/.koolade chat.

mod connect;
pub(crate) mod github;
pub(crate) mod onboarding;

#[cfg(test)]
mod tests;

pub use connect::attempt_connect;
pub use github::{
    GithubTarget, clone_destination, parse_github_url, perform_clone, perform_clone_at,
};
