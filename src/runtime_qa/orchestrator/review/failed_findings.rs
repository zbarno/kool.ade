use crate::runtime_qa::{
    QaDriverActionV1, QaEvidenceManifestV1, QaFindingCategory, QaFindingSeverity, QaFindingV1,
    QaRunRequestV1, QaScenarioV1,
};

pub(in crate::runtime_qa::orchestrator) fn failed_assertion_findings(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    evidence: &QaEvidenceManifestV1,
) -> Vec<QaFindingV1> {
    evidence
        .assertions
        .iter()
        .filter(|assertion| assertion.status == super::super::QaAssertionStatus::Failed)
        .filter_map(|assertion| {
            let scenario_id = assertion.scenario_ids.first()?;
            let scenario = scenarios.iter().find(|item| &item.id == scenario_id)?;
            let screenshot_ids = evidence
                .screenshots
                .iter()
                .filter(|shot| &shot.scenario_id == scenario_id)
                .map(|shot| shot.id.clone())
                .collect();
            let trace_ids = evidence
                .actions
                .iter()
                .filter(|action| &action.scenario_id == scenario_id)
                .map(|action| action.index.to_string())
                .collect();
            Some(QaFindingV1 {
                finding_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
                code_sha: request.code_head_sha.clone(),
                scenario_uid: scenario.id.clone(),
                requirement_uids: assertion.requirement_ids.clone(),
                category: QaFindingCategory::Functional,
                severity: QaFindingSeverity::High,
                expected: assertion.expected.clone(),
                actual: assertion.observed.clone(),
                reproducible_steps: scenario
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(index, step)| {
                        format!("Step {}: {}", index + 1, action_label(&step.driver_action))
                    })
                    .collect(),
                screenshot_ids,
                trace_ids,
                smallest_fix: "Investigate and correct the observed acceptance-criteria mismatch."
                    .into(),
                blocking: true,
            })
        })
        .collect()
}

fn action_label(action: &QaDriverActionV1) -> &'static str {
    use QaDriverActionV1 as Action;
    match action {
        Action::LaunchValidatedTarget => "launch the validated application",
        Action::WaitForWindow => "wait for the application window",
        Action::ListWindows => "inspect available windows",
        Action::FocusWindow { .. } => "focus the application window",
        Action::ResizeWindow { .. } => "resize the application window",
        Action::Click { .. } => "click the application window",
        Action::DoubleClick { .. } => "double-click the application window",
        Action::Drag { .. } => "drag within the application window",
        Action::TypeText { .. } => "type approved test input",
        Action::KeyChord { .. } => "send an approved keyboard chord",
        Action::Scroll { .. } => "scroll the application window",
        Action::WaitForVisible { .. } => "wait for a visible element",
        Action::AssertRegionVisible { .. } => "assert native control region is visible",
        Action::NavigateAllowedUrl { .. } | Action::BrowserOpen { .. } => "open an allowed app URL",
        Action::FindByRole { .. } => "find an element by role",
        Action::FindByTestId { .. } => "find an element by test ID",
        Action::Fill { .. } | Action::BrowserFill { .. } => "fill an approved field",
        Action::Press { .. } | Action::BrowserPress { .. } => "press an approved key",
        Action::Select { .. } | Action::BrowserSelect { .. } => "select an approved option",
        Action::Screenshot | Action::BrowserScreenshot => "capture a screenshot",
        Action::ReadAppLogs => "read application logs",
        Action::CollectConsole => "collect browser console output",
        Action::CollectNetworkFailures => "collect failed browser requests",
        Action::BrowserClick { .. } => "click an approved browser locator",
        Action::BrowserWaitForVisible { .. } => "wait for a browser locator",
        Action::BrowserAssertText { .. } => "assert visible browser text",
        Action::BrowserAssertUrl { .. } => "assert the allowed browser URL",
        Action::BrowserSetViewport { .. } => "set the approved browser viewport",
        Action::BrowserScroll { .. } => "scroll to an approved browser locator",
        Action::ApiRequest { .. } => "send an approved API request through the task-service relay",
        Action::CloseWindow { .. } => "close the application window",
    }
}
