//! Value objects shared across the planner core and the UI.
//! This file only wires submodules; the meat lives beside it.

pub mod artifact_identity;
pub mod chatlog;
pub mod decision;
pub mod item;
pub mod stakeholder;
pub mod user;

pub use artifact_identity::ArtifactIdentity;
pub use chatlog::*;
pub use decision::*;
pub use item::*;
pub use stakeholder::*;
pub use user::*;
