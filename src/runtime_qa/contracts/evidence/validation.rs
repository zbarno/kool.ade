use sha2::Digest;

use super::super::{QaRunRequestV1, QaScenarioV1, is_sha256, is_uuid, valid_text};
use super::{QaAssertionStatus, QaEvidenceManifestV1};

impl QaEvidenceManifestV1 {
    pub fn validate(
        &self,
        request: &QaRunRequestV1,
        scenarios: &[QaScenarioV1],
    ) -> Result<(), String> {
        if self.schema_version != 1
            || self.qa_run_id != request.qa_run_id
            || self.task_uid != request.parent_task_uid
            || self.code_sha != request.code_head_sha
            || self.approved_spec_hash != request.approved_spec_hash
            || self.build_hash != request.build_artifact_sha256
            || !request.relevant_platforms.contains(&self.platform)
            || self.driver_version != request.driver_profile_version
            || !valid_display(self.platform, &self.display)
            || !valid_text(&self.display.theme, 64)
            || !valid_text(&self.display.locale, 64)
            || self.scenario_ids.len() != scenarios.len()
            || self
                .scenario_ids
                .iter()
                .any(|id| !scenarios.iter().any(|scenario| &scenario.id == id))
            || self.actions.len() > request.bounded_limits.max_total_actions as usize
            || self.screenshots.len() > request.bounded_limits.max_screenshots as usize
            || self.assertions.len() > 4096
            || self.logs.len() > 256
            || self.crashes.len() > 256
            || self.browser_console.len() > 4096
            || self.failed_requests.len() > 4096
            || self.coverage.len() > 4096
            || self
                .logs
                .iter()
                .chain(self.crashes.iter())
                .chain(self.browser_console.iter())
                .chain(self.failed_requests.iter())
                .chain(self.coverage.iter())
                .any(|value| !valid_text(value, 4096))
            || !valid_text(&self.retention_policy, 256)
        {
            return Err("runtime QA evidence manifest does not match its request or bounds".into());
        }
        let scenario_ids = self
            .scenario_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        if scenario_ids.len() != scenarios.len()
            || scenarios
                .iter()
                .any(|scenario| !scenario_ids.contains(&&scenario.id))
        {
            return Err("runtime QA evidence must identify each scenario exactly once".into());
        }
        let scenario_by_id = scenarios
            .iter()
            .map(|scenario| (scenario.id.as_str(), scenario))
            .collect::<std::collections::BTreeMap<_, _>>();
        for (expected, action) in self.actions.iter().enumerate() {
            if action.index != expected as u32
                || !scenario_by_id.contains_key(action.scenario_id.as_str())
                || !valid_action_command(&action.command)
                || action.occurred_at_unix_ms == 0
                || action.args_redacted.len() > 64
                || action
                    .args_redacted
                    .iter()
                    .any(|(key, value)| !valid_text(key, 128) || !valid_text(value, 512))
                || action
                    .pre_state
                    .as_ref()
                    .is_some_and(|value| !valid_text(value, 4096))
                || action
                    .post_state
                    .as_ref()
                    .is_some_and(|value| !valid_text(value, 4096))
                || (requires_state_transition(&action.command)
                    && (action.pre_state.is_none() || action.post_state.is_none()))
                || action.evidence_ids.len() > 128
                || action.evidence_ids.iter().any(|id| !is_uuid(id))
            {
                return Err("runtime QA action trace is invalid".into());
            }
        }
        if !self.actions.is_empty()
            && scenarios.iter().any(|scenario| {
                let has_action = self
                    .actions
                    .iter()
                    .any(|action| action.scenario_id == scenario.id);
                let has_blocked_assertion = self.assertions.iter().any(|assertion| {
                    assertion.scenario_ids.contains(&scenario.id)
                        && assertion.status == QaAssertionStatus::Blocked
                });
                !has_action && !has_blocked_assertion
            })
        {
            return Err("runtime QA action trace omits a required scenario".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for screenshot in &self.screenshots {
            if !is_uuid(&screenshot.id)
                || !scenario_by_id.contains_key(screenshot.scenario_id.as_str())
                || !ids.insert(screenshot.id.as_str())
                || !is_sha256(&screenshot.sha256)
                || screenshot.mime != "image/png"
                || screenshot.width == 0
                || screenshot.height == 0
                || self
                    .display
                    .width
                    .is_none_or(|width| screenshot.width > width)
                || self
                    .display
                    .height
                    .is_none_or(|height| screenshot.height > height)
                || screenshot.source_action as usize >= self.actions.len()
                || screenshot.storage_ref
                    != format!(
                        "qa-evidence/{}/screenshots/{}.png",
                        self.qa_run_id, screenshot.id
                    )
                || !safe_evidence_path(&screenshot.storage_ref, &self.qa_run_id)
                || !self
                    .actions
                    .get(screenshot.source_action as usize)
                    .is_some_and(|action| {
                        action.scenario_id == screenshot.scenario_id
                            && action.evidence_ids.contains(&screenshot.id)
                    })
                || screenshot.redaction_masks.len() > 256
                || screenshot
                    .redaction_masks
                    .iter()
                    .any(|mask| !valid_text(mask, 512))
            {
                return Err("runtime QA screenshot metadata is invalid".into());
            }
        }
        for action in &self.actions {
            for evidence_id in &action.evidence_ids {
                ids.insert(evidence_id.as_str());
            }
        }
        for assertion in &self.assertions {
            if !is_uuid(&assertion.id)
                || assertion.scenario_ids.is_empty()
                || assertion.scenario_ids.len() > scenarios.len()
                || assertion
                    .scenario_ids
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != assertion.scenario_ids.len()
                || assertion
                    .scenario_ids
                    .iter()
                    .any(|id| !scenario_by_id.contains_key(id.as_str()))
                || assertion.requirement_ids.is_empty()
                || assertion
                    .requirement_ids
                    .iter()
                    .any(|id| !request.requirement_ids.contains(id))
                || !valid_text(&assertion.expected, 4096)
                || !valid_text(&assertion.observed, 4096)
                || (matches!(
                    assertion.status,
                    QaAssertionStatus::Passed | QaAssertionStatus::Failed
                ) && assertion.evidence_refs.is_empty())
                || assertion
                    .evidence_refs
                    .iter()
                    .any(|id| !ids.contains(id.as_str()))
                || assertion.requirement_ids.iter().any(|requirement| {
                    !assertion.scenario_ids.iter().any(|scenario_id| {
                        scenario_by_id
                            .get(scenario_id.as_str())
                            .is_some_and(|scenario| scenario.requirement_ids.contains(requirement))
                    })
                })
            {
                return Err(
                    "runtime QA assertion is incomplete or references unknown evidence".into(),
                );
            }
        }
        for action in &self.actions {
            if action
                .evidence_ids
                .iter()
                .any(|id| !ids.contains(id.as_str()))
            {
                return Err("runtime QA action trace references unknown evidence".into());
            }
        }
        for requirement_id in &request.requirement_ids {
            if !self
                .assertions
                .iter()
                .any(|assertion| assertion.requirement_ids.contains(requirement_id))
            {
                return Err(format!(
                    "runtime QA requirement {requirement_id} lacks an explicit assertion status"
                ));
            }
        }
        Ok(())
    }

    pub fn sha256(&self) -> Result<String, String> {
        self.validate_shape()?;
        let bytes =
            serde_json::to_vec(self).map_err(|_| "runtime QA evidence cannot be encoded")?;
        Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
    }

    fn validate_shape(&self) -> Result<(), String> {
        if self.schema_version != 1 || !is_uuid(&self.qa_run_id) {
            return Err("runtime QA evidence manifest identity is invalid".into());
        }
        Ok(())
    }
}

fn valid_display(platform: super::super::QaPlatform, display: &super::super::QaDisplayV1) -> bool {
    match platform {
        super::super::QaPlatform::HeadlessService => {
            display.width.is_none()
                && display.height.is_none()
                && display.dpi.is_none()
                && display.theme == "headless-service"
        }
        super::super::QaPlatform::LinuxDesktop | super::super::QaPlatform::Web => {
            display
                .width
                .is_some_and(|width| (320..=7680).contains(&width))
                && display
                    .height
                    .is_some_and(|height| (240..=4320).contains(&height))
                && display.dpi.is_some_and(|dpi| (48..=960).contains(&dpi))
        }
    }
}

fn safe_evidence_path(path: &str, run_id: &str) -> bool {
    path.starts_with(&format!("qa-evidence/{run_id}/"))
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains('\\'))
        && path.len() <= 1024
}

fn valid_action_command(command: &str) -> bool {
    matches!(
        command,
        "api_request"
            | "launch_validated_target"
            | "wait_for_window"
            | "list_windows"
            | "focus_window"
            | "resize_window"
            | "click"
            | "double_click"
            | "drag"
            | "type_text"
            | "key_chord"
            | "scroll"
            | "wait_for_visible"
            | "navigate_allowed_url"
            | "find_by_role"
            | "find_by_test_id"
            | "fill"
            | "press"
            | "select"
            | "screenshot"
            | "read_app_logs"
            | "collect_console"
            | "collect_network_failures"
            | "open"
            | "wait_for"
            | "evaluate_limited_assertions"
            | "set_viewport"
            | "close"
    )
}

fn requires_state_transition(command: &str) -> bool {
    matches!(
        command,
        "api_request"
            | "click"
            | "double_click"
            | "drag"
            | "type_text"
            | "key_chord"
            | "fill"
            | "press"
            | "select"
    )
}
