use super::super::*;

pub(super) struct FinalizationContext<'a> {
    pub docker: &'a bollard::Docker,
    pub request: &'a crate::runtime_qa::QaRunRequestV1,
    pub scenario: QaScenarioV1,
    pub preview: &'a crate::execution::docker::qa_container::QaWebPreview,
    pub driver_result: Result<crate::runtime_qa::QaDriverOutputV1, String>,
    pub origin_escape_result: Result<(), String>,
    pub seeded_defect_result: Result<(), String>,
    pub cancellation_result: Result<(), String>,
    pub concurrent_runs_result: Result<(), String>,
    pub preview_failure_result: Result<(), String>,
    pub headless_service_result: Option<Result<crate::runtime_qa::QaDriverOutputV1, String>>,
    pub evidence_root: &'a std::path::Path,
}

pub(super) async fn finish(context: FinalizationContext<'_>) {
    super::super::cleanup::cleanup_and_assert(
        context.docker,
        context.request,
        None,
        context.preview,
    )
    .await;
    context
        .origin_escape_result
        .unwrap_or_else(|error| panic!("origin escape was not blocked: {error}"));
    context
        .seeded_defect_result
        .unwrap_or_else(|error| panic!("seeded browser defect was not reported: {error}"));
    context
        .cancellation_result
        .unwrap_or_else(|error| panic!("cancelled browser run was not cleaned: {error}"));
    context
        .concurrent_runs_result
        .unwrap_or_else(|error| panic!("concurrent browser runs were not isolated: {error}"));
    context
        .preview_failure_result
        .unwrap_or_else(|error| panic!("preview process failure was not handled safely: {error}"));

    let output = context.driver_result.unwrap();
    assert!(output.completed);
    assert_eq!(output.assertions.len(), 2);
    assert!(
        output
            .assertions
            .iter()
            .all(|assertion| { assertion.status == crate::runtime_qa::QaAssertionStatus::Passed })
    );
    assert!(output.screenshots.iter().any(|screen| screen.width >= 320));
    if let Some(result) = context.headless_service_result {
        let service_output =
            result.unwrap_or_else(|error| panic!("headless service QA failed: {error}"));
        assert!(service_output.completed);
        assert_eq!(service_output.assertions.len(), 1);
        assert_eq!(
            service_output.assertions[0].status,
            crate::runtime_qa::QaAssertionStatus::Passed
        );
        let service_evidence = serde_json::to_string(&service_output).unwrap();
        assert!(!service_evidence.contains("headless fixture input"));
        assert!(!service_evidence.contains(r#"{"saved":true}"#));
    }
    super::super::lifecycle::review_and_persist(
        context.request,
        context.scenario,
        &context.preview.service_lease,
        output,
        context.evidence_root.join("qa-evidence"),
    );
}
