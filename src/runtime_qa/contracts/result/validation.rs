use super::super::{
    QaAssertionStatus, QaEvidenceManifestV1, QaOutcome, QaPlatform, QaRunRequestV1, QaScenarioV1,
    is_uuid, scenario_set_sha256, valid_text,
};
use super::{CURRENT_QA_RESULT_SCHEMA_VERSION, QaCaseStatus, QaResultV1, QaReviewCoverageStatusV1};

mod review;
use review::validate_review_coverage;

impl QaResultV1 {
    pub fn validate(
        &self,
        request: &QaRunRequestV1,
        scenarios: &[QaScenarioV1],
        manifest: &QaEvidenceManifestV1,
    ) -> Result<(), String> {
        manifest.validate(request, scenarios)?;
        let evidence_hash = manifest.sha256()?;
        let approved_scenario_hash = scenario_set_sha256(scenarios)?;
        if !matches!(
            self.schema_version,
            1 | 2 | 3 | CURRENT_QA_RESULT_SCHEMA_VERSION
        ) || self.subject_sha != request.code_head_sha
            || self.run_id != request.qa_run_id
            || !is_uuid(&self.reviewer_execution_id)
            || !is_uuid(&self.reviewer_session_id)
            || self.reviewer_execution_id == request.author_run_id
            || self.reviewer_session_id == request.author_session_id
            || self.evidence_manifest_sha256 != evidence_hash
            || (self.schema_version == CURRENT_QA_RESULT_SCHEMA_VERSION
                && self.scenario_set_sha256 != approved_scenario_hash)
            || (manifest.actions.is_empty()
                && (self.outcome != QaOutcome::InfrastructureBlocked
                    || self
                        .cases
                        .iter()
                        .any(|case| case.status != QaCaseStatus::Blocked)))
            || self.cases.len() != scenarios.len()
            || self.coverage.requirements.len() != request.requirement_ids.len()
            || self.coverage.scenario_ids.len() != scenarios.len()
            || self.coverage.states.len() > 512
            || self
                .coverage
                .states
                .iter()
                .any(|state| !valid_text(state, 256))
            || self.started_at_unix_ms == 0
            || self.completed_at_unix_ms < self.started_at_unix_ms
            || self.tool_versions.is_empty()
            || self.tool_versions.len() > 128
            || self
                .tool_versions
                .iter()
                .any(|(key, value)| !valid_text(key, 128) || !valid_text(value, 128))
            || self.tool_versions.get("driver") != Some(&manifest.driver_version)
        {
            return Err("runtime QA result does not match its immutable request/evidence".into());
        }
        let legacy_without_review = matches!(self.schema_version, 1 | 2)
            && self.reviewed_screenshot_ids.is_empty()
            && self.review_coverage.is_empty();
        let known_evidence = manifest
            .screenshots
            .iter()
            .map(|shot| shot.id.as_str())
            .chain(
                manifest
                    .actions
                    .iter()
                    .flat_map(|action| action.evidence_ids.iter().map(String::as_str)),
            )
            .collect::<std::collections::BTreeSet<_>>();
        let reviewed_screenshots = self
            .reviewed_screenshot_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        if reviewed_screenshots.len() != self.reviewed_screenshot_ids.len()
            || self.reviewed_screenshot_ids.iter().any(|id| {
                !manifest
                    .screenshots
                    .iter()
                    .any(|screenshot| &screenshot.id == id)
            })
        {
            return Err("runtime QA reviewed screenshot IDs are missing or invalid".into());
        }
        if !legacy_without_review {
            validate_review_coverage(self, request, scenarios, manifest, &reviewed_screenshots)?;
        }
        let case_ids = self
            .cases
            .iter()
            .map(|case| case.scenario_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let covered_scenario_ids = self
            .coverage
            .scenario_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        if case_ids.len() != self.cases.len()
            || scenarios
                .iter()
                .any(|scenario| !case_ids.contains(scenario.id.as_str()))
            || covered_scenario_ids.len() != scenarios.len()
            || scenarios
                .iter()
                .any(|scenario| !covered_scenario_ids.contains(&&scenario.id))
            || self.cases.iter().any(|case| {
                case.evidence_refs
                    .iter()
                    .any(|id| !known_evidence.contains(id.as_str()))
                    || (matches!(case.status, QaCaseStatus::Passed | QaCaseStatus::Failed)
                        && case.evidence_refs.is_empty())
                    || case.observations.len() > 128
                    || case.observations.iter().any(|item| !valid_text(item, 4096))
            })
        {
            return Err("runtime QA case coverage or evidence references are invalid".into());
        }
        let requirement_ids = self
            .coverage
            .requirements
            .iter()
            .map(|item| item.requirement_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if requirement_ids.len() != request.requirement_ids.len()
            || request
                .requirement_ids
                .iter()
                .any(|id| !requirement_ids.contains(id.as_str()))
            || self.coverage.requirements.iter().any(|item| {
                item.status == super::QaCoverageStatus::Observed
                    && (item.evidence_refs.is_empty()
                        || item
                            .evidence_refs
                            .iter()
                            .any(|id| !known_evidence.contains(id.as_str())))
            })
            || self
                .findings
                .iter()
                .any(|finding| finding.validate(manifest).is_err())
        {
            return Err("runtime QA requirement coverage or finding evidence is invalid".into());
        }
        let headless_service =
            request.relevant_platforms.as_slice() == [QaPlatform::HeadlessService];
        let requires_screenshot_review = !headless_service
            || scenarios.iter().any(|scenario| {
                !scenario.visual_expectations.is_empty()
                    && scenario
                        .requirement_ids
                        .iter()
                        .any(|id| request.requirement_ids.contains(id))
            });
        if self.outcome == QaOutcome::Pass
            && (self
                .cases
                .iter()
                .any(|case| case.status != QaCaseStatus::Passed)
                || self
                    .coverage
                    .requirements
                    .iter()
                    .any(|item| item.status != super::QaCoverageStatus::Observed)
                || (requires_screenshot_review && manifest.screenshots.is_empty())
                || (!legacy_without_review
                    && ((requires_screenshot_review && self.reviewed_screenshot_ids.is_empty())
                        || self.review_coverage.len() != request.requirement_ids.len()
                        || self
                            .review_coverage
                            .iter()
                            .any(|item| item.status != QaReviewCoverageStatusV1::Pass)))
                || manifest
                    .assertions
                    .iter()
                    .any(|item| item.status != QaAssertionStatus::Passed)
                || self.coverage.states.is_empty()
                || request.requirement_ids.iter().any(|requirement| {
                    !manifest.assertions.iter().any(|assertion| {
                        assertion.status == QaAssertionStatus::Passed
                            && assertion.requirement_ids.iter().any(|id| id == requirement)
                    })
                })
                || self.findings.iter().any(|finding| finding.blocking))
        {
            return Err(
                "runtime QA cannot pass with failed, untested, blocked, or blocking evidence"
                    .into(),
            );
        }
        if self.outcome == QaOutcome::Fail
            && !self
                .cases
                .iter()
                .any(|case| case.status == QaCaseStatus::Failed)
            && !manifest
                .assertions
                .iter()
                .any(|assertion| assertion.status == QaAssertionStatus::Failed)
            && self.findings.iter().all(|finding| !finding.blocking)
            && manifest.crashes.is_empty()
        {
            return Err("runtime QA cannot report failure without an observed failing assertion, case, finding, or crash".into());
        }
        if self.outcome == QaOutcome::InfrastructureBlocked
            && !self
                .cases
                .iter()
                .any(|case| case.status == QaCaseStatus::Blocked)
            && !self
                .coverage
                .requirements
                .iter()
                .any(|coverage| coverage.status == super::QaCoverageStatus::Blocked)
            && !manifest
                .assertions
                .iter()
                .any(|assertion| assertion.status == QaAssertionStatus::Blocked)
        {
            return Err(
                "runtime QA infrastructure block requires explicit blocked evidence".into(),
            );
        }
        if self.findings.len() > 512 {
            return Err("runtime QA result exceeds report limits".into());
        }
        Ok(())
    }
}
