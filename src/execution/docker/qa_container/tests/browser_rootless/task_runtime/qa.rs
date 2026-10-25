use super::super::*;
use std::path::Path;

#[path = "qa/headless.rs"]
mod headless;
#[path = "qa/web.rs"]
mod web;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExpectedBrowserOutcome {
    Pass,
    Fail,
}

pub(super) fn scenario(request: &crate::runtime_qa::QaRunRequestV1) -> QaScenarioV1 {
    if request.relevant_platforms.as_slice() == [crate::runtime_qa::QaPlatform::HeadlessService] {
        let mut steps = vec![step(
            QaDriverActionV1::ApiRequest {
                method: QaHttpMethodV1::Post,
                path: "/save".into(),
                json_body: Some(r#"{"message":"synthetic task QA input"}"#.into()),
                expected_status: 200,
                expected_body_contains: Some("\"saved\":true".into()),
            },
            "The API reports a successful save",
            false,
        )];
        if request.test_data_fixture_ref.as_deref() == Some("go-headless-api-v1") {
            steps.push(step(
                QaDriverActionV1::ApiRequest {
                    method: QaHttpMethodV1::Get,
                    path: "/runner-protection".into(),
                    json_body: None,
                    expected_status: 200,
                    expected_body_contains: Some("\"protected\":true".into()),
                },
                "The same-UID fixture process cannot read or truncate its sealed readiness runner",
                false,
            ));
        }
        return QaScenarioV1 {
            schema_version: 1,
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            requirement_ids: request.requirement_ids.clone(),
            preconditions: vec!["The approved API service is running.".into()],
            steps,
            expected_results: vec![request.acceptance_criteria[0].clone()],
            negative_paths: Vec::new(),
            visual_expectations: Vec::new(),
            redaction_regions: Vec::new(),
            priority: 1,
        };
    }
    QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec![
            "The approved project build, test, and start receipts pass for this task revision."
                .into(),
        ],
        steps: vec![
            step(
                QaDriverActionV1::BrowserOpen {
                    url: "http://qa-service/".into(),
                },
                "The task-built form is visible",
                true,
            ),
            step(
                QaDriverActionV1::BrowserFill {
                    locator: QaBrowserLocatorV1::Label {
                        label: "Message".into(),
                    },
                    value: "synthetic task QA input".into(),
                },
                "The synthetic message is in the form",
                false,
            ),
            step(
                QaDriverActionV1::BrowserPress {
                    locator: QaBrowserLocatorV1::Label {
                        label: "Message".into(),
                    },
                    key: "Enter".into(),
                },
                "The task-built application accepts the form submission",
                false,
            ),
            step(
                QaDriverActionV1::BrowserAssertText {
                    locator: QaBrowserLocatorV1::TestId {
                        test_id: "status".into(),
                    },
                    expected_text: "Saved".into(),
                },
                "The saved state is visible",
                true,
            ),
        ],
        expected_results: vec![
            "The approved startup artifact serves the task form.".into(),
            "Submitting a non-empty message displays Saved.".into(),
        ],
        negative_paths: Vec::new(),
        // The fixture reviewer below verifies functional evidence only. The
        // pixel-aware visual review contract is exercised by its own tests.
        visual_expectations: Vec::new(),
        redaction_regions: Vec::new(),
        priority: 1,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    docker: &bollard::Docker,
    lock: &ImageLock,
    browser_image: &crate::execution::docker::images::SelectedImage,
    service_image: &crate::execution::docker::images::SelectedImage,
    request: &crate::runtime_qa::QaRunRequestV1,
    scenario: &QaScenarioV1,
    recipe: &crate::project_runtime::ExecutionRecipeV1,
    artifact_upload: &QaContainerUpload,
    artifact_root: &Path,
    evidence_root: &Path,
    readiness_path: &str,
    expected_browser_outcome: ExpectedBrowserOutcome,
) {
    if request.relevant_platforms.as_slice() == [crate::runtime_qa::QaPlatform::HeadlessService] {
        assert_eq!(expected_browser_outcome, ExpectedBrowserOutcome::Pass);
        headless::run(
            docker,
            lock,
            browser_image,
            service_image,
            request,
            scenario,
            recipe,
            artifact_upload,
            artifact_root,
            evidence_root,
        );
        return;
    }
    web::run(
        docker,
        lock,
        browser_image,
        service_image,
        request,
        scenario,
        recipe,
        artifact_upload,
        artifact_root,
        evidence_root,
        readiness_path,
        expected_browser_outcome,
    );
}

fn step(
    action: QaDriverActionV1,
    expected_state: &str,
    screenshot_checkpoint: bool,
) -> QaScenarioStepV1 {
    QaScenarioStepV1 {
        driver_action: action,
        expected_state: Some(expected_state.into()),
        screenshot_checkpoint,
    }
}
