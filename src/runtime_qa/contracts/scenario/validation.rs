use super::QaScenarioV1;

pub fn validate_scenario_set(
    request: &super::super::QaRunRequestV1,
    scenarios: &[QaScenarioV1],
) -> Result<(), String> {
    request.validate()?;
    if scenarios.is_empty() || scenarios.len() > request.bounded_limits.max_scenarios as usize {
        return Err("runtime QA scenario count exceeds the request bounds".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for scenario in scenarios {
        scenario.validate(request)?;
        if !ids.insert(scenario.id.as_str()) {
            return Err("runtime QA scenario IDs must be unique".into());
        }
    }
    let covered = scenarios
        .iter()
        .flat_map(|scenario| scenario.requirement_ids.iter())
        .collect::<std::collections::BTreeSet<_>>();
    if request
        .requirement_ids
        .iter()
        .any(|id| !covered.contains(id))
    {
        return Err("runtime QA scenarios do not cover every approved requirement".into());
    }
    let total_actions: usize = scenarios.iter().map(|scenario| scenario.steps.len()).sum();
    if total_actions > request.bounded_limits.max_total_actions as usize {
        return Err("runtime QA total action count exceeds the request bounds".into());
    }
    Ok(())
}
