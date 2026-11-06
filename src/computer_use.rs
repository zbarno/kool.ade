//! Contracts for isolated graphical Computer Use sessions.
//!
//! These contracts intentionally do not share authority with Docker execution.
//! They describe data exchanged with a future VM controller and guest companion.

mod contract;
pub mod guest_image;

pub use contract::*;
