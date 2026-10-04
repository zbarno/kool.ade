//! Resumable task generation with per-artifact validation and bounded repair.
//! Checkpoints and immutable attempt evidence are private runtime data, not tasks.
mod generation;
pub(crate) mod prompt;
mod repair;
pub(super) mod response;
mod run;
pub use generation::generate;

#[cfg(test)]
mod tests;
