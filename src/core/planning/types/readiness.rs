use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ReadinessAxis {
    Who,
    What,
    When,
    Where,
    Why,
    How,
    OutOfScope,
    CriticalPath,
    SystemBoundaries,
    FailureBehavior,
    DataOwnership,
    Authorization,
    MeasurableSuccess,
    TestCases,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct AcceptanceTestTrace {
    pub requirement_id: String,
    pub test_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ReadinessChecklistV1 {
    pub feature_uid: String,
    pub plan_revision: u64,
    /// SHA-256 revision of the immutable feature snapshot assessed by Planner.
    /// Missing legacy bindings are not eligible for plan confirmation.
    #[serde(default)]
    pub spec_revision: Option<String>,
    /// Complete requirement set parsed from the assessed specification.
    /// Missing legacy values fail readiness closed until Planner reassesses it.
    #[serde(default)]
    pub required_requirement_ids: Option<Vec<String>>,
    pub assessed_axes: Vec<ReadinessAxis>,
    pub acceptance_tests: Vec<AcceptanceTestTrace>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub unresolved_blocker_ids: Vec<String>,
    #[serde(default)]
    pub blocker_notes: Vec<String>,
    pub ready: bool,
    pub stalled: bool,
    pub no_progress_count: u32,
    pub progress_fingerprint: Option<String>,
    pub revision: u64,
}

impl ReadinessChecklistV1 {
    pub fn validate(&self) -> Result<(), String> {
        if uuid::Uuid::parse_str(&self.feature_uid).is_err()
            || self.assessed_axes.len() > MATERIAL_AXIS_COUNT
            || self.acceptance_tests.len() > 2048
            || self
                .required_requirement_ids
                .as_ref()
                .is_some_and(|requirements| {
                    requirements.len() > 256
                        || requirements.iter().any(|id| {
                            id.strip_prefix("REQ-").is_none_or(|suffix| {
                                suffix.is_empty()
                                    || !suffix.chars().all(|character| {
                                        character.is_ascii_alphanumeric() || character == '-'
                                    })
                            })
                        })
                })
            || self.evidence_refs.len() > 128
            || self
                .evidence_refs
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 128)
            || self.unresolved_blocker_ids.len() > 2048
            || self.blocker_notes.len() > 128
            || self
                .blocker_notes
                .iter()
                .any(|note| note.trim().is_empty() || note.len() > 4096)
            || self.no_progress_count > 1_000_000
            || self.progress_fingerprint.as_ref().is_some_and(|value| {
                value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            || self.spec_revision.as_ref().is_some_and(|value| {
                value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            || self.acceptance_tests.iter().any(|entry| {
                entry.requirement_id.trim().is_empty()
                    || entry.requirement_id.len() > 256
                    || entry.test_ref.trim().is_empty()
                    || entry.test_ref.len() > 2048
            })
            || self
                .unresolved_blocker_ids
                .iter()
                .any(|uid| uuid::Uuid::parse_str(uid).is_err())
        {
            return Err("readiness checklist is invalid or exceeds limits".into());
        }
        if self
            .assessed_axes
            .iter()
            .enumerate()
            .any(|(index, axis)| self.assessed_axes[..index].contains(axis))
            || self
                .acceptance_tests
                .iter()
                .enumerate()
                .any(|(index, entry)| {
                    self.acceptance_tests[..index]
                        .iter()
                        .any(|prior| prior == entry)
                })
            || self
                .required_requirement_ids
                .as_ref()
                .is_some_and(|requirements| {
                    requirements
                        .iter()
                        .enumerate()
                        .any(|(index, id)| requirements[..index].contains(id))
                })
            || self
                .unresolved_blocker_ids
                .iter()
                .enumerate()
                .any(|(index, uid)| self.unresolved_blocker_ids[..index].contains(uid))
        {
            return Err("readiness checklist contains duplicate evidence".into());
        }
        if self.ready
            && (self.acceptance_tests.is_empty()
                || !self.unresolved_blocker_ids.is_empty()
                || !self.blocker_notes.is_empty()
                || self.stalled)
        {
            return Err(
                "ready checklists require acceptance tests and no blockers or stall".into(),
            );
        }
        Ok(())
    }
}

const MATERIAL_AXIS_COUNT: usize = 14;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadinessResult {
    Ready,
    InvalidEvidence {
        reasons: Vec<String>,
    },
    NeedsWork {
        blockers: Vec<String>,
        unassessed: Vec<ReadinessAxis>,
        untraced_requirements: Vec<String>,
        missing_acceptance_tests: bool,
    },
    PlanningStalled {
        repeated_fingerprint: String,
    },
}
