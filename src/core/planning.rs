//! Typed clarification questions and deterministic planning readiness.
mod ambiguity;
mod deferral;
mod questions;
mod readiness;
mod resolution;
mod store;
mod types;

pub use ambiguity::{
    QuestionKey, decline_question, normalize_decision_key, propose_questions, reconcile_answer,
    supersede_question,
};
pub use deferral::defer_question;
pub use questions::{QuestionContent, question_content};
pub use readiness::{
    DEFAULT_NO_PROGRESS_THRESHOLD, MATERIAL_AXES, assess_readiness, observe_progress,
};
pub use resolution::{EvidenceResolution, resolve_question_with_evidence};
pub use store::QuestionStore;
pub use types::*;

#[cfg(test)]
mod tests;
