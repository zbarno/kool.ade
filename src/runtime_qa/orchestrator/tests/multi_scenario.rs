use super::*;

#[test]
// Production boundary: a later scenario may fail before its first action
// after an earlier scenario has already produced evidence. The orchestrator
// must persist both the observed trace and the blocked case. In-process driver
// and temporary store only; no Docker, network, or credentials are required.
fn blocked_scenario_without_actions_keeps_prior_scenario_evidence() {
    let (request, mut scenarios, lease) = fixture();
    let mut blocked_scenario = scenarios[0].clone();
    blocked_scenario.id = uuid::Uuid::new_v4().hyphenated().to_string();
    scenarios.push(blocked_scenario);
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Passed,
        failure_before_actions: false,
        failure_on_call: Some(2),
    };
    let (result, _) = RuntimeQaOrchestrator::new(&store)
        .execute(QaExecutionContext {
            request: &request,
            scenarios: &scenarios,
            authorization: QaTargetAuthorization::Web(&lease),
            driver: &mut driver,
            reviewer_execution_id: &reviewer_run,
            reviewer_session_id: &reviewer_session,
            prompt,
        })
        .unwrap();

    assert_eq!(result.outcome, QaOutcome::InfrastructureBlocked);
    assert_eq!(result.cases[0].status, QaCaseStatus::Passed);
    assert_eq!(result.cases[1].status, QaCaseStatus::Blocked);
    let (stored_request, stored_scenarios, evidence, stored_result) =
        store.load_run(&request.qa_run_id).unwrap();
    assert_eq!(stored_request, request);
    assert_eq!(stored_scenarios, scenarios);
    assert_eq!(evidence.actions.len(), 2);
    assert!(
        evidence
            .actions
            .iter()
            .all(|action| { action.scenario_id == scenarios[0].id })
    );
    assert_eq!(stored_result, result);
}
