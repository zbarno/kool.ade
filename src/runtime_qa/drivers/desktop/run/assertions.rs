use crate::runtime_qa::{QaAssertionStatus, QaAssertionV1, QaDriverOutputV1, QaScenarioV1};

pub(super) fn record_expected(
    output: &mut QaDriverOutputV1,
    scenario: &QaScenarioV1,
    expected: Option<&str>,
    observed: Option<String>,
    evidence_ids: &mut Vec<String>,
) {
    let (Some(expected), Some(observed)) = (expected, observed) else {
        return;
    };
    let passed = super::expected_state_met(expected, &observed);
    output.assertions.push(QaAssertionV1 {
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        scenario_ids: vec![scenario.id.clone()],
        requirement_ids: scenario.requirement_ids.clone(),
        expected: expected.into(),
        observed,
        // An observed mismatch is a functional failure. Reserve Untested for
        // expectations the driver could not observe.
        status: if passed {
            QaAssertionStatus::Passed
        } else {
            QaAssertionStatus::Failed
        },
        evidence_refs: std::mem::take(evidence_ids),
    });
}
