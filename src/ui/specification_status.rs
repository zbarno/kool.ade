#[derive(Clone, Debug, Default)]
pub struct FeatureSpecificationStatus {
    pub feature_uid: Option<String>,
    pub current_spec_hash: Option<String>,
    pub drift_status: &'static str,
    pub approved_target_hash: Option<String>,
    pub revision: Option<u64>,
    pub reconciliation_status: Option<crate::core::lifecycle::SpecReconciliationStatusV1>,
    pub outcome_reason: Option<String>,
    pub pending_triggers: usize,
    pub reviewed_heads: Vec<(String, String)>,
    pub merged_commits: Vec<(String, String)>,
    pub current_implementation: Option<crate::core::lifecycle::CurrentImplementationEvidenceV1>,
    pub current_implementation_stale: bool,
    pub deployment_status: Option<String>,
    pub deployment_observed_at_ms: Option<u64>,
    pub deployed_spec_hash: Option<String>,
    pub feature_deltas: Vec<crate::core::lifecycle::SpecDeltaRecordV1>,
}
