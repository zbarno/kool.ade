use super::*;

pub(super) fn browser_scenario(request: &crate::runtime_qa::QaRunRequestV1) -> QaScenarioV1 {
    QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved Python preview is running.".into()],
        steps: vec![
            step(
                QaDriverActionV1::BrowserOpen {
                    url: "http://qa-service/__cache_ready".into(),
                },
                "The service cache probe is visible",
                false,
            ),
            step(
                QaDriverActionV1::BrowserAssertText {
                    locator: QaBrowserLocatorV1::TestId {
                        test_id: "cache-status".into(),
                    },
                    expected_text: "available".into(),
                },
                "The service process can read its approved npm cache",
                false,
            ),
            step(
                QaDriverActionV1::BrowserOpen {
                    url: "http://qa-service/".into(),
                },
                "Preview fixture is visible",
                true,
            ),
            step(
                QaDriverActionV1::BrowserFill {
                    locator: QaBrowserLocatorV1::Label {
                        label: "Message".into(),
                    },
                    value: "rootless synthetic input".into(),
                },
                "Message field contains synthetic input",
                false,
            ),
            step(
                QaDriverActionV1::BrowserPress {
                    locator: QaBrowserLocatorV1::Label {
                        label: "Message".into(),
                    },
                    key: "Enter".into(),
                },
                "Form submission completes",
                false,
            ),
            step(
                QaDriverActionV1::BrowserAssertText {
                    locator: QaBrowserLocatorV1::TestId {
                        test_id: "status".into(),
                    },
                    expected_text: "Saved".into(),
                },
                "Saved state is observed",
                true,
            ),
        ],
        expected_results: vec![
            "The approved service process can read its staged npm cache.".into(),
            "Submitting the form updates the page to Saved.".into(),
        ],
        negative_paths: vec![],
        visual_expectations: vec!["The fixture form and status are visible.".into()],
        redaction_regions: vec![],
        priority: 1,
    }
}
