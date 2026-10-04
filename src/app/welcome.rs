//! First-launch / connect screen: pick a git repository, validate it,
//! bootstrap planning artifacts if absent, and hydrate the ~/.koolade chat.

mod connect;
mod github;
mod repository_picker;
mod screen;

#[cfg(test)]
mod tests;

pub use connect::attempt_connect;
pub use github::{
    GithubTarget, clone_destination, parse_github_url, perform_clone, perform_clone_at,
};
pub use screen::paint;
