use super::super::*;
use crate::persistence::persona::{self, PersonaLoad, SHIPPED_DEFAULT_PERSONA};
use std::time::Duration;
mod support;
use support::{Contested, EnvClaim, JOURNEY_ATTEMPTS};
mod disk;
mod mapping;
fn fixture(
    document: &str,
    seeded_now: bool,
    fell_back: bool,
    diagnostic: Option<String>,
) -> PersonaLoad {
    PersonaLoad {
        document: document.to_owned(),
        seeded_now,
        fell_back_to_default: fell_back,
        diagnostic,
    }
}
