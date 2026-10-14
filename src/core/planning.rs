//! Typed clarification questions and deterministic planning readiness.
mod ambiguity;
mod questions;
mod readiness;
mod store;
mod types;

pub use ambiguity::{
    QuestionKey, decline_question, normalize_decision_key, propose_questions, reconcile_answer,
};
pub use questions::{QuestionContent, question_content};
pub use readiness::{MATERIAL_AXES, assess_readiness, observe_progress};
pub use store::QuestionStore;
pub use types::*;

#[cfg(test)]
mod tests;
