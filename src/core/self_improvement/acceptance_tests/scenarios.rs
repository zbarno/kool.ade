use super::{OpportunityScenario, fixture::run_opportunity_acceptance};

#[test]
fn unsupported_language_opportunity_uses_explicit_pursuit_and_persisted_lifecycle_gates() {
    run_opportunity_acceptance(OpportunityScenario::UnsupportedLanguage);
}

#[test]
fn unrelated_ux_opportunity_uses_explicit_pursuit_and_persisted_lifecycle_gates() {
    run_opportunity_acceptance(OpportunityScenario::UnrelatedUx);
}
