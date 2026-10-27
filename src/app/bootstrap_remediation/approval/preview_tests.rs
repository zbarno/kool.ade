use std::collections::BTreeSet;

use super::{PlanConsent, all_required_consents_selected};

#[test]
fn each_additional_consent_must_be_selected_before_approval() {
    let required = [
        PlanConsent::RootfulDockerPackageActivation,
        PlanConsent::RootfulDockerCoexistence,
    ];
    let mut selected = BTreeSet::new();
    assert!(!all_required_consents_selected(&required, &selected));

    selected.insert(PlanConsent::RootfulDockerPackageActivation);
    assert!(!all_required_consents_selected(&required, &selected));

    selected.insert(PlanConsent::RootfulDockerCoexistence);
    assert!(all_required_consents_selected(&required, &selected));
}
