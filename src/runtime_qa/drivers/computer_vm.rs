// Production selection is owned by the Runtime QA app dispatcher. Guest
// authority remains the authenticated Computer Use controller session.

mod action_contract;

use std::time::{Duration, Instant};

use sha2::Digest as _;

use crate::runtime_qa::{
    QaActionEvidenceV1, QaAssertionStatus, QaAssertionV1, QaComputerTargetV1, QaDisplayV1,
    QaDriverActionV1, QaDriverOutputV1, QaDriverScreenshotV1, QaRunRequestV1, QaScenarioV1,
    QaTargetAuthorization,
};
use action_contract::{action_name, now_ms, supports_generic_computer_action};

/// The verified Computer Use adapter implements these operations over one
/// authenticated, consented disposable guest session. Implementations must
/// verify each P15 receipt and current lease before staging or interacting;
/// guest observations are untrusted data.
pub(crate) trait ComputerQaGuestSessionV1 {
    fn begin_scenario(
        &mut self,
        target: &QaComputerTargetV1,
        scenario_id: &str,
        deadline: Instant,
    ) -> Result<ComputerQaDisplayV1, String>;

    fn launch_staged_application(
        &mut self,
        target: &QaComputerTargetV1,
        deadline: Instant,
    ) -> Result<(), String>;

    fn observe_state(&mut self, deadline: Instant) -> Result<String, String>;

    /// Execute only a typed, approved GUI operation. No shell or arbitrary
    /// executable arguments are representable through this interface.
    fn perform_action(
        &mut self,
        action: &QaDriverActionV1,
        deadline: Instant,
    ) -> Result<(), String>;

    fn screenshot_png(&mut self, deadline: Instant) -> Result<Vec<u8>, String>;

    fn end_scenario(&mut self) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ComputerQaDisplayV1 {
    pub width: u16,
    pub height: u16,
    pub dpi: u16,
}

impl ComputerQaDisplayV1 {
    fn validate_for(self, target: &QaComputerTargetV1) -> Result<(), String> {
        if self.width != target.display_width
            || self.height != target.display_height
            || self.dpi != target.display_dpi
        {
            return Err(
                "Computer QA guest display differs from the approved session profile".into(),
            );
        }
        Ok(())
    }

    fn evidence(self) -> QaDisplayV1 {
        QaDisplayV1 {
            width: Some(self.width),
            height: Some(self.height),
            dpi: Some(self.dpi),
            theme: "guest-default".into(),
            locale: "C.UTF-8".into(),
        }
    }
}

/// Adapts Computer Use guest actions into the existing Runtime QA evidence,
/// review, and repair lifecycle.
pub(crate) struct ComputerQaScenarioDriverV1<'a, G> {
    pub request: &'a QaRunRequestV1,
    pub target: &'a QaComputerTargetV1,
    pub guest: &'a mut G,
}

impl<G: ComputerQaGuestSessionV1> crate::runtime_qa::QaScenarioDriver
    for ComputerQaScenarioDriverV1<'_, G>
{
    fn platform(&self) -> crate::runtime_qa::QaPlatform {
        crate::runtime_qa::QaPlatform::LinuxDesktop
    }

    fn supports_computer_qa(&self) -> bool {
        true
    }

    fn run_scenario(
        &mut self,
        request: &QaRunRequestV1,
        scenario: &QaScenarioV1,
        authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String> {
        if request != self.request
            || !matches!(authorization, QaTargetAuthorization::ComputerVm(target) if *target == self.target)
        {
            return Err(
                "Computer QA driver received a mismatched request or guest authorization".into(),
            );
        }
        request.validate()?;
        scenario.validate(request)?;
        self.target.validate_for(request)?;

        let deadline =
            Instant::now() + Duration::from_secs(u64::from(request.bounded_limits.timeout_seconds));
        let display = self
            .guest
            .begin_scenario(self.target, &scenario.id, deadline)?;
        display.validate_for(self.target)?;
        let result = run_scenario_steps(
            self.guest,
            request,
            scenario,
            self.target,
            display,
            deadline,
        );
        let cleanup = self.guest.end_scenario();
        match (result, cleanup) {
            (Ok(output), Ok(())) => Ok(output),
            (Ok(mut output), Err(error)) => {
                output.completed = false;
                output.failure = Some(format!("Computer QA guest cleanup failed: {error}"));
                Ok(output)
            }
            (Err(error), _) => Err(error),
        }
    }
}

fn run_scenario_steps<G: ComputerQaGuestSessionV1>(
    guest: &mut G,
    request: &QaRunRequestV1,
    scenario: &QaScenarioV1,
    target: &QaComputerTargetV1,
    display: ComputerQaDisplayV1,
    deadline: Instant,
) -> Result<QaDriverOutputV1, String> {
    let mut output = QaDriverOutputV1 {
        driver_version: request.driver_profile_version.clone(),
        completed: false,
        failure: None,
        display: display.evidence(),
        actions: Vec::new(),
        screenshots: Vec::new(),
        assertions: Vec::new(),
        logs: Vec::new(),
        crashes: Vec::new(),
        browser_console: Vec::new(),
        failed_requests: Vec::new(),
    };
    let mut launched = false;
    let mut screenshot_bytes = 0u64;
    for (index, step) in scenario.steps.iter().enumerate() {
        if Instant::now() >= deadline {
            output.failure = Some("Computer QA scenario exceeded its approved timeout".into());
            break;
        }
        let before = guest.observe_state(deadline)?;
        if matches!(step.driver_action, QaDriverActionV1::LaunchValidatedTarget) {
            if launched {
                output.failure =
                    Some("approved Computer QA target was launched more than once".into());
                break;
            }
            guest.launch_staged_application(target, deadline)?;
            launched = true;
        } else if !launched {
            output.failure =
                Some("Computer QA scenario must launch the approved build first".into());
            break;
        } else if !matches!(step.driver_action, QaDriverActionV1::Screenshot) {
            if !supports_generic_computer_action(&step.driver_action) {
                return Err(
                    "Computer QA does not support this action through generic guest controls"
                        .into(),
                );
            }
            guest.perform_action(&step.driver_action, deadline)?;
        }
        let after = guest.observe_state(deadline)?;
        let expected_mismatch = step
            .expected_state
            .as_ref()
            .is_some_and(|expected| !after.to_lowercase().contains(&expected.to_lowercase()));
        let capture = matches!(step.driver_action, QaDriverActionV1::Screenshot)
            || step.screenshot_checkpoint
            || expected_mismatch;
        let screenshot_id = if capture {
            if output.screenshots.len() >= usize::from(request.bounded_limits.max_screenshots) {
                output.failure = Some("Computer QA exceeded its approved screenshot count".into());
                break;
            }
            let bytes = guest.screenshot_png(deadline)?;
            screenshot_bytes = screenshot_bytes.saturating_add(bytes.len() as u64);
            if screenshot_bytes > request.bounded_limits.max_evidence_bytes {
                output.failure = Some(
                    "Computer QA screenshots exceeded the approved evidence byte limit".into(),
                );
                break;
            }
            let id = uuid::Uuid::new_v4().hyphenated().to_string();
            let sha256 = format!("{:x}", sha2::Sha256::digest(&bytes));
            output.screenshots.push(QaDriverScreenshotV1 {
                id: id.clone(),
                scenario_id: scenario.id.clone(),
                sha256,
                width: display.width,
                height: display.height,
                source_action: index as u32,
                redaction_masks: Vec::new(),
                bytes,
            });
            Some(id)
        } else {
            None
        };
        let evidence_ids = screenshot_id.into_iter().collect::<Vec<_>>();
        output.actions.push(QaActionEvidenceV1 {
            index: index as u32,
            scenario_id: scenario.id.clone(),
            command: action_name(&step.driver_action)?,
            args_redacted: Default::default(),
            pre_state: Some(before),
            post_state: Some(after.clone()),
            evidence_ids: evidence_ids.clone(),
            occurred_at_unix_ms: now_ms(),
        });
        if let Some(expected) = step.expected_state.as_deref() {
            let passed = after.to_lowercase().contains(&expected.to_lowercase());
            output.assertions.push(QaAssertionV1 {
                id: uuid::Uuid::new_v4().hyphenated().to_string(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: scenario.requirement_ids.clone(),
                expected: expected.into(),
                observed: after,
                status: if passed {
                    QaAssertionStatus::Passed
                } else {
                    QaAssertionStatus::Failed
                },
                evidence_refs: evidence_ids,
            });
        }
    }
    if output.failure.is_none() {
        output.completed = output.actions.len() == scenario.steps.len();
    }
    if output.assertions.is_empty()
        && let Some(screenshot) = output.screenshots.last()
    {
        for (index, requirement) in scenario.requirement_ids.iter().enumerate() {
            output.assertions.push(QaAssertionV1 {
                id: uuid::Uuid::new_v4().hyphenated().to_string(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: vec![requirement.clone()],
                expected: request
                    .acceptance_criteria
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| scenario.expected_results[0].clone()),
                observed: "Screenshot retained for independent review; no deterministic assertion matched this criterion".into(),
                status: QaAssertionStatus::Untested,
                evidence_refs: vec![screenshot.id.clone()],
            });
        }
    }
    Ok(output)
}
