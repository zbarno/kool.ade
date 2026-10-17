use super::evidence_resolution_events;
use crate::harness::prompt_catalog::TechnicalResolutionV1;

fn resolution(uid: &str) -> TechnicalResolutionV1 {
    TechnicalResolutionV1 {
        question_uid: uid.into(),
        decision: "Use the existing repository API".into(),
        rationale: "The current implementation and tests establish this behavior.".into(),
        selected_option_id: Some("existing-api".into()),
        evidence_refs: vec!["repository-overview:src/api.rs".into()],
    }
}

#[test]
fn technical_evidence_resolution_queues_spec_update_without_authorizing_work() {
    let events = evidence_resolution_events(&[resolution("question-1")]);
    let event = &events[0];

    assert!(event.contains("living specification's technical decisions/assumptions"));
    assert!(event.contains("question-1: Use the existing repository API"));
    assert!(event.contains("repository-overview:src/api.rs"));
    assert!(event.contains("preserve user-owned decisions and approved scope"));
    assert!(event.contains("Do not approve or implement work"));
}

#[test]
fn technical_evidence_resolution_event_is_bounded() {
    let resolutions = (0..20)
        .map(|index| {
            let mut item = resolution(&format!("question-{index}"));
            item.decision = "x".repeat(2_000);
            item.rationale = "y".repeat(2_000);
            item
        })
        .collect::<Vec<_>>();

    let events = evidence_resolution_events(&resolutions);
    assert_eq!(events.len(), 3);
    assert!(events.iter().all(|event| event.len() < 24_000));
    for index in 0..20 {
        assert!(
            events
                .iter()
                .any(|event| event.contains(&format!("question-{index}")))
        );
    }
}

#[test]
fn no_technical_resolutions_queue_no_spec_update() {
    assert!(evidence_resolution_events(&[]).is_empty());
}
