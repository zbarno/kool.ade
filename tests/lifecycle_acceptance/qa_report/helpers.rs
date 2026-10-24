use koolade::runtime_qa::QaActionEvidenceV1;
use std::collections::BTreeMap;

pub(super) fn action(
    index: u32,
    scenario_id: &str,
    command: &str,
    evidence: Option<String>,
) -> QaActionEvidenceV1 {
    QaActionEvidenceV1 {
        index,
        scenario_id: scenario_id.into(),
        command: command.into(),
        args_redacted: BTreeMap::new(),
        pre_state: Some("fixture page loaded".into()),
        post_state: Some("fixture interaction recorded".into()),
        evidence_ids: evidence.into_iter().collect(),
        occurred_at_unix_ms: u64::from(index) + 1,
    }
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
