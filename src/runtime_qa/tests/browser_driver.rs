use super::*;

#[test]
fn headless_service_scenarios_are_bounded_api_only_and_nonvisual() {
    let mut request = request();
    request.relevant_platforms = vec![QaPlatform::HeadlessService];
    let mut scenario = scenario(&request);
    scenario.steps = vec![QaScenarioStepV1 {
        driver_action: QaDriverActionV1::ApiRequest {
            method: QaHttpMethodV1::Post,
            path: "/v1/items".into(),
            json_body: Some(r#"{"name":"fixture item"}"#.into()),
            expected_status: 201,
            expected_body_contains: Some("fixture item".into()),
        },
        expected_state: Some("the fixture item is created".into()),
        screenshot_checkpoint: false,
    }];
    scenario.visual_expectations.clear();
    assert!(scenario.validate(&request).is_ok());
    let json = super::super::browser_driver::build_service_request(&request, &scenario).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(value["max_screenshots"], 0);
    assert_eq!(value["actions"][0]["action"], "api_request");
    assert_eq!(value["actions"][0]["path"], "/v1/items");

    scenario.steps[0].driver_action = QaDriverActionV1::ApiRequest {
        method: QaHttpMethodV1::Get,
        path: "http://outside.invalid/".into(),
        json_body: None,
        expected_status: 200,
        expected_body_contains: None,
    };
    assert!(scenario.validate(&request).is_err());
    for path in [
        "/./private",
        "/%2e/private",
        "/../private",
        "/%2e%2e/private",
        "/.%2e/private",
        "/%2e./private",
        "/%2E%2e/private",
    ] {
        scenario.steps[0].driver_action = QaDriverActionV1::ApiRequest {
            method: QaHttpMethodV1::Get,
            path: path.into(),
            json_body: None,
            expected_status: 200,
            expected_body_contains: None,
        };
        assert!(scenario.validate(&request).is_err(), "accepted path {path}");
    }
    scenario.steps[0].driver_action = QaDriverActionV1::ApiRequest {
        method: QaHttpMethodV1::Get,
        path: "/items?token=secret".into(),
        json_body: None,
        expected_status: 200,
        expected_body_contains: None,
    };
    assert!(scenario.validate(&request).is_err());
    scenario.steps[0].driver_action = QaDriverActionV1::ApiRequest {
        method: QaHttpMethodV1::Get,
        path: "/items".into(),
        json_body: Some(r#"{"unexpected":"GET body"}"#.into()),
        expected_status: 200,
        expected_body_contains: None,
    };
    assert!(scenario.validate(&request).is_err());
    scenario.visual_expectations.push("blue button".into());
    assert!(scenario.validate(&request).is_err());
    scenario.visual_expectations.clear();
    scenario.steps[0].screenshot_checkpoint = true;
    assert!(scenario.validate(&request).is_err());
}

#[test]
fn service_driver_report_binds_status_and_response_hash_without_screenshots() {
    let mut request = request();
    request.relevant_platforms = vec![QaPlatform::HeadlessService];
    let mut scenario = scenario(&request);
    scenario.steps = vec![QaScenarioStepV1 {
        driver_action: QaDriverActionV1::ApiRequest {
            method: QaHttpMethodV1::Get,
            path: "/health".into(),
            json_body: None,
            expected_status: 200,
            expected_body_contains: Some("\"status\":\"ok\"".into()),
        },
        expected_state: Some("service health is ok".into()),
        screenshot_checkpoint: false,
    }];
    scenario.visual_expectations.clear();
    let evidence_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let report = serde_json::json!({
        "schema_version": 1,
        "qa_run_id": request.qa_run_id,
        "scenario_id": scenario.id,
        "task_uid": request.parent_task_uid,
        "code_sha": request.code_head_sha,
        "build_sha256": request.build_artifact_sha256,
        "approved_spec_hash": request.approved_spec_hash,
        "driver_profile_version": request.driver_profile_version,
        "outcome": "completed",
        "origin": "http://qa-preview:8080",
        "viewport": {"width":1280,"height":720},
        "driver_version": "node-pinned",
        "actions": [{
            "index":0,
            "action":"api_request",
            "args_redacted":{"method":"GET","path":"/health"},
            "pre_state":{"url":"http://qa-preview:8080","title":"request pending"},
            "post_state":{"url":"http://qa-preview:8080/health","title":"status=200; sha256={}; body_match=true".replace("{}", &"a".repeat(64))},
            "evidence_ids":[evidence_id],
            "response_status":200,
            "response_sha256":"a".repeat(64),
            "body_matches":true,
            "status":"observed"
        }],
        "screenshots": [],
        "console_errors": [],
        "failed_requests": [],
        "policy_violation": null
    });
    let output = super::super::browser_driver::parse_browser_output(
        &serde_json::to_vec(&report).unwrap(),
        &request,
        &scenario,
    )
    .unwrap();
    assert!(output.completed);
    assert!(output.screenshots.is_empty());
    assert_eq!(output.assertions[0].status, QaAssertionStatus::Passed);
    assert_eq!(output.assertions[0].evidence_refs, vec![evidence_id]);
    assert_eq!(output.display.width, None);
    assert_eq!(output.display.theme, "headless-service");

    let mut wrong_status = scenario.clone();
    if let QaDriverActionV1::ApiRequest {
        expected_status, ..
    } = &mut wrong_status.steps[0].driver_action
    {
        *expected_status = 201;
    }
    let failed = super::super::browser_driver::parse_browser_output(
        &serde_json::to_vec(&report).unwrap(),
        &request,
        &wrong_status,
    )
    .unwrap();
    assert_eq!(failed.assertions[0].status, QaAssertionStatus::Failed);

    let mut partial_scenario = scenario.clone();
    partial_scenario.steps.push(QaScenarioStepV1 {
        driver_action: QaDriverActionV1::ApiRequest {
            method: QaHttpMethodV1::Get,
            path: "/items".into(),
            json_body: None,
            expected_status: 200,
            expected_body_contains: None,
        },
        expected_state: Some("items are returned".into()),
        screenshot_checkpoint: false,
    });
    let mut partial_report = report.clone();
    partial_report["outcome"] = "failed".into();
    partial_report["actions"][0]["status"] = "failed".into();
    partial_report["actions"][0]
        .as_object_mut()
        .unwrap()
        .remove("response_status");
    partial_report["actions"][0]
        .as_object_mut()
        .unwrap()
        .remove("response_sha256");
    partial_report["actions"][0]
        .as_object_mut()
        .unwrap()
        .remove("body_matches");
    let partial = super::super::browser_driver::parse_browser_output(
        &serde_json::to_vec(&partial_report).unwrap(),
        &request,
        &partial_scenario,
    )
    .unwrap();
    assert_eq!(partial.assertions.len(), 2);
    assert_eq!(partial.assertions[0].status, QaAssertionStatus::Failed);
    assert_eq!(partial.assertions[1].status, QaAssertionStatus::Blocked);

    let mut forged_report = report;
    forged_report["actions"][0]["args_redacted"]["path"] = "/other".into();
    assert!(
        super::super::browser_driver::parse_browser_output(
            &serde_json::to_vec(&forged_report).unwrap(),
            &request,
            &scenario,
        )
        .is_err()
    );
}

#[test]
fn browser_scenarios_use_typed_locators_and_preview_service_urls() {
    let request = request();
    let mut scenario = scenario(&request);
    scenario.steps = vec![
        QaScenarioStepV1 {
            driver_action: QaDriverActionV1::BrowserOpen {
                url: "http://qa-service/".into(),
            },
            expected_state: None,
            screenshot_checkpoint: true,
        },
        QaScenarioStepV1 {
            driver_action: QaDriverActionV1::BrowserClick {
                locator: QaBrowserLocatorV1::Role {
                    role: "button".into(),
                    name: "Save message".into(),
                },
            },
            expected_state: Some("save result is visible".into()),
            screenshot_checkpoint: true,
        },
    ];
    assert!(scenario.validate(&request).is_ok());
    scenario.steps[0].driver_action = QaDriverActionV1::BrowserOpen {
        url: "http://127.0.0.1:4173/".into(),
    };
    assert!(scenario.validate(&request).is_err());
}

#[test]
fn linux_desktop_session_rejects_browser_only_actions() {
    let session = LinuxDesktopSessionV1 {
        qa_run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_width: 1024,
        display_height: 768,
        windows: vec![],
    };
    assert!(
        session
            .validate_action(&QaDriverActionV1::BrowserScreenshot)
            .is_err()
    );
}

#[test]
fn browser_request_adapter_keeps_typed_actions_on_the_exact_relay_origin() {
    let request = request();
    let mut scenario = scenario(&request);
    scenario.steps = vec![QaScenarioStepV1 {
        driver_action: QaDriverActionV1::BrowserOpen {
            url: "http://qa-service/profile".into(),
        },
        expected_state: None,
        screenshot_checkpoint: true,
    }];
    let json = super::super::browser_driver::build_browser_request(&request, &scenario).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(value["origin"], "http://qa-preview:8080");
    assert_eq!(value["actions"][0]["action"], "browser_open");
    assert_eq!(value["actions"][0]["screenshot_checkpoint"], true);
    assert_eq!(value["max_screenshots"], 16);

    scenario.steps[0].driver_action = QaDriverActionV1::NavigateAllowedUrl {
        url: "http://qa-service/profile".into(),
    };
    assert!(super::super::browser_driver::build_browser_request(&request, &scenario).is_err());
}

#[test]
fn browser_driver_report_is_bound_to_the_request_and_relay_origin() {
    let request = request();
    let mut scenario = scenario(&request);
    scenario.steps = vec![QaScenarioStepV1 {
        driver_action: QaDriverActionV1::BrowserOpen {
            url: "http://qa-service/".into(),
        },
        expected_state: None,
        screenshot_checkpoint: false,
    }];
    let report = serde_json::json!({
        "schema_version": 1,
        "qa_run_id": request.qa_run_id.clone(),
        "scenario_id": scenario.id.clone(),
        "task_uid": request.parent_task_uid.clone(),
        "code_sha": request.code_head_sha.clone(),
        "build_sha256": request.build_artifact_sha256.clone(),
        "approved_spec_hash": request.approved_spec_hash.clone(),
        "driver_profile_version": request.driver_profile_version.clone(),
        "outcome": "completed",
        "origin": "http://qa-preview:8080",
        "viewport": { "width": 1280, "height": 720 },
        "driver_version": "playwright-143.0.7499.4",
        "actions": [{
            "index": 0,
            "action": "browser_open",
            "args_redacted": {},
            "pre_state": { "url": "about:blank", "title": "" },
            "post_state": { "url": "http://qa-preview:8080/", "title": "Preview" },
            "screenshot_ids": [],
            "status": "observed"
        }],
        "screenshots": [],
        "console_errors": [],
        "failed_requests": [],
        "policy_violation": null
    });
    let bytes = serde_json::to_vec(&report).unwrap();
    let output =
        super::super::browser_driver::parse_browser_output(&bytes, &request, &scenario).unwrap();
    assert!(output.completed);
    assert_eq!(output.actions.len(), 1);
    assert_eq!(output.actions[0].command, "open");

    let mut escaped = report;
    escaped["actions"][0]["post_state"]["url"] = "http://127.0.0.1:4173/".into();
    let bytes = serde_json::to_vec(&escaped).unwrap();
    assert!(
        super::super::browser_driver::parse_browser_output(&bytes, &request, &scenario).is_err()
    );
}

#[test]
fn failed_browser_interaction_is_a_test_failure_not_infrastructure_block() {
    let request = request();
    let mut scenario = scenario(&request);
    scenario.steps = vec![QaScenarioStepV1 {
        driver_action: QaDriverActionV1::BrowserClick {
            locator: QaBrowserLocatorV1::Role {
                role: "button".into(),
                name: "Save".into(),
            },
        },
        expected_state: Some("saved confirmation is visible".into()),
        screenshot_checkpoint: true,
    }];
    let report = serde_json::json!({
        "schema_version": 1,
        "qa_run_id": request.qa_run_id,
        "scenario_id": scenario.id,
        "task_uid": request.parent_task_uid,
        "code_sha": request.code_head_sha,
        "build_sha256": request.build_artifact_sha256,
        "approved_spec_hash": request.approved_spec_hash,
        "driver_profile_version": request.driver_profile_version,
        "outcome": "failed",
        "origin": "http://qa-preview:8080",
        "viewport": { "width": 1280, "height": 720 },
        "driver_version": "playwright-pinned",
        "actions": [{
            "index": 0,
            "action": "browser_click",
            "args_redacted": {"locator": {"locator": "role", "role": "button", "name": "Save"}},
            "pre_state": {"url": "http://qa-preview:8080/", "title": "Preview"},
            "post_state": {"url": "http://qa-preview:8080/", "title": "Preview"},
            "screenshot_ids": [],
            "status": "failed"
        }],
        "screenshots": [],
        "console_errors": [],
        "failed_requests": [],
        "policy_violation": null,
        "error": "locator click timed out"
    });
    let bytes = serde_json::to_vec(&report).unwrap();

    let output =
        super::super::browser_driver::parse_browser_output(&bytes, &request, &scenario).unwrap();

    assert!(output.completed);
    assert!(output.failure.is_none());
    assert_eq!(output.assertions.len(), 1);
    assert_eq!(output.assertions[0].status, QaAssertionStatus::Failed);
}
