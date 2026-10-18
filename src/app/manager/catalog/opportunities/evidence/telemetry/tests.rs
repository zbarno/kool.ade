use super::*;

fn invocation(at: DateTime<Utc>, outcome: &str) -> InvocationRecord {
    let mut record = InvocationRecord::new("synthetic-project", "synthetic-harness");
    record.ended_at = Some(at);
    record.outcome = outcome.into();
    record
}

#[test]
fn typed_local_telemetry_emits_only_a_sufficiently_supported_regression() {
    let now = DateTime::parse_from_rfc3339("2026-10-17T12:00:00Z")
        .unwrap()
        .to_utc();
    let mut records = Vec::new();
    for (offset, outcome) in ["failed", "completed", "completed", "completed"]
        .into_iter()
        .enumerate()
    {
        records.push(invocation(
            now - Duration::days(10) + Duration::minutes(offset as i64),
            outcome,
        ));
    }
    for (offset, outcome) in ["failed", "failed", "timed-out", "completed"]
        .into_iter()
        .enumerate()
    {
        records.push(invocation(
            now - Duration::days(2) + Duration::minutes(offset as i64),
            outcome,
        ));
    }
    records.push(invocation(now - Duration::days(1), "cancelled"));
    records.push(invocation(now - Duration::days(1), "unknown"));

    let evidence =
        telemetry_regression_evidence("00000000-0000-4000-8000-000000000001", &records, now)
            .unwrap();

    assert!(evidence.id.starts_with("telemetry-regression:"));
    assert_eq!(
        evidence.confidentiality,
        EvidenceConfidentiality::WorkspacePrivate
    );
    assert!(
        evidence
            .content
            .contains("baseline_7d=4 failures=1 rate=25%")
    );
    assert!(evidence.content.contains("recent_7d=4 failures=3 rate=75%"));
    assert!(
        evidence
            .content
            .contains("regression_delta=50 percentage_points")
    );
}

#[test]
fn telemetry_noise_and_small_samples_do_not_create_regression_evidence() {
    let now = Utc::now();
    let records = [
        invocation(now - Duration::days(10), "failed"),
        invocation(now - Duration::days(10), "completed"),
        invocation(now - Duration::days(2), "failed"),
        invocation(now - Duration::days(2), "completed"),
        invocation(now - Duration::days(2), "cancelled"),
    ];

    assert!(telemetry_regression_evidence("workspace", &records, now).is_none());
}
