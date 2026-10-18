//! User-facing opportunity and self-improvement path for #174/#175.
//! The proposal is inert until persisted Pursue; active capability readiness is
//! outside this fixture. It covers store gates and isolated local Git clone only.
#[path = "acceptance_tests/fixture.rs"]
mod fixture;
#[path = "acceptance_tests/scenarios.rs"]
mod scenarios;
#[path = "acceptance_support.rs"]
mod support;

#[derive(Clone, Copy)]
enum OpportunityScenario {
    UnsupportedLanguage,
    UnrelatedUx,
}
