use std::time::{SystemTime, UNIX_EPOCH};

mod report;
mod review;
#[cfg(test)]
#[path = "orchestrator/tests.rs"]
mod tests;
use report::{ReportAssembly, assemble_report};
use review::{apply_review_verdict, failed_assertion_findings};

use crate::harness::prompt_catalog::{PromptManifest, PromptRole, validate_prompt_manifest};

use super::{
    QaActionEvidenceV1, QaAssertionStatus, QaCaseResultV1, QaCaseStatus, QaCoverageStatus,
    QaDisplayV1, QaDriverOutputV1, QaEvidenceManifestV1, QaOutcome, QaPlatform,
    QaRequirementCoverageV1, QaResultV1, QaRunRequestV1, QaScenarioV1, QaScreenshotArtifactV1,
    QaScreenshotV1, QaTaskServiceLeaseV1, QaValidatedTargetV1, RuntimeQaStore,
    approved_gate_receipt, validate_scenario_set,
};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

/// App-resolved target authorization. Scenario text never supplies executable
/// paths or service network identities.
pub enum QaTargetAuthorization<'a> {
    LinuxDesktop(&'a QaValidatedTargetV1),
    HeadlessService(&'a QaTaskServiceLeaseV1),
    Web(&'a QaTaskServiceLeaseV1),
}

/// Trusted, app-owned adapter for one supported QA driver. Implementations must
/// execute only the supplied immutable request and validated scenario.
pub trait QaScenarioDriver {
    fn platform(&self) -> QaPlatform;

    fn run_scenario(
        &mut self,
        request: &QaRunRequestV1,
        scenario: &QaScenarioV1,
        authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String>;
}

pub struct QaExecutionContext<'a> {
    pub request: &'a QaRunRequestV1,
    pub scenarios: &'a [QaScenarioV1],
    pub authorization: QaTargetAuthorization<'a>,
    pub driver: &'a mut dyn QaScenarioDriver,
    pub reviewer_execution_id: &'a str,
    pub reviewer_session_id: &'a str,
    pub prompt: PromptManifest,
}

pub type QaReviewer<'a> = dyn FnMut(
        &QaEvidenceManifestV1,
        &QaResultV1,
    ) -> Result<crate::harness::prompt_catalog::ReviewVerdictV1, String>
    + 'a;

/// Coordinates one immutable QA run. Unsupported platforms and untrusted
/// reviewer identities fail closed before any driver is invoked.
pub struct RuntimeQaOrchestrator<'a> {
    store: &'a RuntimeQaStore,
}

impl<'a> RuntimeQaOrchestrator<'a> {
    pub fn new(store: &'a RuntimeQaStore) -> Self {
        Self { store }
    }

    #[cfg(test)]
    pub fn execute(
        &self,
        context: QaExecutionContext<'_>,
    ) -> Result<(QaResultV1, Option<crate::core::lifecycle::GateReceipt>), String> {
        self.execute_inner(context, None)
    }

    /// Production entrypoint: a pass is eligible only after the selected
    /// independent reviewer has examined the observed evidence and verdict.
    pub fn execute_reviewed(
        &self,
        context: QaExecutionContext<'_>,
        reviewer: &mut QaReviewer<'_>,
    ) -> Result<(QaResultV1, Option<crate::core::lifecycle::GateReceipt>), String> {
        self.execute_inner(context, Some(reviewer))
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_inner(
        &self,
        context: QaExecutionContext<'_>,
        mut reviewer: Option<&mut QaReviewer<'_>>,
    ) -> Result<(QaResultV1, Option<crate::core::lifecycle::GateReceipt>), String> {
        let QaExecutionContext {
            request,
            scenarios,
            authorization,
            driver,
            reviewer_execution_id,
            reviewer_session_id,
            prompt,
        } = context;
        request.validate()?;
        validate_scenario_set(request, scenarios)?;
        match (&authorization, driver.platform()) {
            (QaTargetAuthorization::LinuxDesktop(target), QaPlatform::LinuxDesktop) => {
                target.validate_for(request)?;
            }
            (QaTargetAuthorization::HeadlessService(lease), QaPlatform::HeadlessService) => {
                lease.validate_for(request)?;
            }
            (QaTargetAuthorization::Web(lease), QaPlatform::Web) => lease.validate_for(request)?,
            _ => return Err("QA target authorization does not match the selected driver".into()),
        }
        let role = validate_prompt_manifest(&prompt)?;
        if !matches!(
            role,
            PromptRole::RuntimeQaReviewer | PromptRole::RuntimeQaReviewerV2
        ) || reviewer_execution_id == request.author_run_id
            || reviewer_session_id == request.author_session_id
            || !crate::runtime_qa::contracts::is_uuid(reviewer_execution_id)
            || !crate::runtime_qa::contracts::is_uuid(reviewer_session_id)
        {
            return Err("runtime QA requires a distinct execution and session using the approved reviewer prompt".into());
        }
        if request.relevant_platforms.len() != 1
            || request.relevant_platforms[0] != driver.platform()
        {
            return Err("no app-owned QA driver supports the exact requested platform set".into());
        }

        let started_at_unix_ms = now_ms();
        let mut outputs = Vec::with_capacity(scenarios.len());
        for scenario in scenarios {
            let output = driver.run_scenario(request, scenario, &authorization)?;
            if output.driver_version != request.driver_profile_version {
                return Err("QA driver version does not match the approved profile".into());
            }
            outputs.push(output);
        }
        let completed_at_unix_ms = now_ms().max(started_at_unix_ms);
        let report = assemble_report(request, scenarios, outputs)?;
        let ReportAssembly {
            mut evidence,
            screenshots,
            cases,
            coverage,
            outcome,
        } = report;
        let findings = failed_assertion_findings(request, scenarios, &evidence);
        for finding in &findings {
            finding.validate(&evidence)?;
        }
        let mut result = QaResultV1 {
            schema_version: 1,
            subject_sha: request.code_head_sha.clone(),
            run_id: request.qa_run_id.clone(),
            reviewer_execution_id: reviewer_execution_id.to_owned(),
            reviewer_session_id: reviewer_session_id.to_owned(),
            outcome,
            evidence_manifest_sha256: evidence.sha256()?,
            cases,
            coverage,
            findings,
            tool_versions: [("driver".into(), evidence.driver_version.clone())]
                .into_iter()
                .collect(),
            started_at_unix_ms,
            completed_at_unix_ms,
        };
        if let Some(review) = reviewer.as_mut() {
            let candidate = result.clone();
            let reviewed = review(&evidence, &candidate).and_then(|verdict| {
                apply_review_verdict(request, scenarios, &evidence, &mut result, verdict)
            });
            if reviewed.is_err() {
                result = candidate;
                result.outcome = QaOutcome::InfrastructureBlocked;
                for requirement in &mut result.coverage.requirements {
                    requirement.status = QaCoverageStatus::Blocked;
                }
                evidence.coverage = result
                    .coverage
                    .requirements
                    .iter()
                    .map(|coverage| format!("{}:{:?}", coverage.requirement_id, coverage.status))
                    .collect();
                evidence.logs.truncate(255);
                evidence.logs.push(
                    "independent runtime QA review could not be completed; result is infrastructure blocked".into(),
                );
                result.evidence_manifest_sha256 = evidence.sha256()?;
            }
        }
        super::validate_report(request, scenarios, &evidence, &result)?;
        // Constructing the gate here verifies that the exact independent role
        // and exact report produce a lifecycle-compatible receipt.
        let receipt = match result.outcome {
            QaOutcome::Pass | QaOutcome::Fail => Some(approved_gate_receipt(
                request,
                scenarios,
                &evidence,
                &result,
                prompt,
                completed_at_unix_ms,
            )?),
            QaOutcome::Incomplete | QaOutcome::InfrastructureBlocked => None,
        };
        self.store.save_run_with_screenshots(
            request,
            scenarios,
            &evidence,
            &result,
            &screenshots,
        )?;
        if let Some(receipt) = &receipt {
            self.store.save_gate_receipt(receipt)?;
        }
        Ok((result, receipt))
    }
}
