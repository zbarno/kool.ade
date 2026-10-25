use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::app::setup_attention::catalog::{
    Finding, FindingStatus, MachineSnapshot, SCHEMA_VERSION as SNAPSHOT_VERSION,
};

mod attempts;
mod reconciliation;
mod transitions;
mod validation;

pub use transitions::SetupTaskTransitionV1;

pub(super) const FILE_SCHEMA_VERSION: u8 = 2;
const TASK_SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupTaskStateV1 {
    Detected,
    ReadyToFix,
    BlockedByPrerequisite,
    NeedsApproval,
    InProgress,
    AwaitingVerification,
    NeedsVerification,
    Verified,
    Failed,
    Unsupported,
    Cancelled,
    Paused,
    Interrupted,
    Superseded,
}

pub type State = SetupTaskStateV1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
    NeedsApproval,
    InProgress,
    AwaitingVerification,
    Verified,
    Failed,
    Cancelled,
    Paused,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    OperationFailed,
    VerificationFailed,
    PermissionDenied,
    CancelledByUser,
    InterruptedByRestart,
    SupersededByReadiness,
    PrerequisiteRegressed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AuthorizationBindingV1 {
    /// Opaque receipt identifier; this is not a credential or bearer token.
    pub receipt_id: String,
    /// Digest binds the exact typed operation; it contains no private inputs.
    pub scope_digest: String,
    pub authorized_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VerifiedResultV1 {
    /// Stable readiness domain, such as `docker` or `images:general:amd64`.
    pub evidence_domain: String,
    pub scan_generation: u64,
    pub verified_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetupDependencyV1 {
    pub prerequisite_id: String,
    pub resolution: DependencyResolution,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub enum DependencyResolution {
    Pending,
    SatisfiedByTask {
        scan_generation: u64,
    },
    SatisfiedByProbe {
        evidence_domain: String,
        scan_generation: u64,
        verified_at_unix_ms: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetupAttemptV1 {
    pub operation_id: String,
    /// Version of the typed operation contract, not executable text.
    pub version: u16,
    /// UUID of the live application instance that owns this active attempt.
    /// It is cleared when the attempt terminates or startup recovery interrupts it.
    #[serde(default)]
    pub owner_instance_id: Option<String>,
    /// Stable prerequisite IDs only; never shell arguments or environment data.
    pub requested_scopes: Vec<String>,
    /// Active metadata is cleared at every terminal state or restart.
    pub authorization_binding: Option<AuthorizationBindingV1>,
    /// Non-authoritative receipt audit record retained after active binding clears.
    pub authorization_history: Option<AuthorizationBindingV1>,
    pub state: AttemptState,
    pub started_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub verified_result: Option<VerifiedResultV1>,
    /// Enumerated, redacted failure information; raw diagnostics are not stored.
    pub failure: Option<FailureCode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetupTaskEventV1 {
    pub observed_at_unix_ms: u64,
    pub scan_generation: u64,
    pub state: SetupTaskStateV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetupTaskV1 {
    pub schema_version: u8,
    pub id: String,
    pub title: String,
    pub explanation: String,
    pub next_step: String,
    pub dependencies: Vec<SetupDependencyV1>,
    /// Current unresolved dependency IDs, derived from the validated DAG.
    pub blocked_by: Vec<String>,
    pub state: SetupTaskStateV1,
    pub created_at_unix_ms: u64,
    pub observed_at_unix_ms: u64,
    pub scan_generation: u64,
    /// Latest successful verification time, retained as historical information.
    pub verified_at_unix_ms: Option<u64>,
    /// Present only while the current readiness state is positively verified.
    pub verified_result: Option<VerifiedResultV1>,
    pub attempts: Vec<SetupAttemptV1>,
    pub events: Vec<SetupTaskEventV1>,
    /// Present only for records migrated from the old generic setup cards.
    pub legacy_source_id: Option<String>,
    pub superseded_by: Vec<String>,
}

pub type LocalTask = SetupTaskV1;

impl SetupTaskV1 {
    pub(super) fn from_finding(finding: &Finding, generation: u64, at: u64) -> Self {
        let state = match finding.status {
            FindingStatus::NeedsAttention => SetupTaskStateV1::Detected,
            FindingStatus::BlockedByPrerequisite => SetupTaskStateV1::BlockedByPrerequisite,
            FindingStatus::Unverified => SetupTaskStateV1::NeedsVerification,
            FindingStatus::Unsupported => SetupTaskStateV1::Unsupported,
        };
        Self {
            schema_version: TASK_SCHEMA_VERSION,
            id: finding.id.clone(),
            title: finding.title.clone(),
            explanation: finding.explanation.clone(),
            next_step: finding.next_step.clone(),
            dependencies: finding
                .prerequisites
                .iter()
                .map(|prerequisite_id| SetupDependencyV1 {
                    prerequisite_id: prerequisite_id.clone(),
                    resolution: DependencyResolution::Pending,
                })
                .collect(),
            blocked_by: Vec::new(),
            state,
            created_at_unix_ms: at,
            observed_at_unix_ms: at,
            scan_generation: generation,
            verified_at_unix_ms: None,
            verified_result: None,
            attempts: Vec::new(),
            events: vec![SetupTaskEventV1 {
                observed_at_unix_ms: at,
                scan_generation: generation,
                state,
            }],
            legacy_source_id: None,
            superseded_by: Vec::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        !matches!(
            self.state,
            SetupTaskStateV1::Verified | SetupTaskStateV1::Superseded
        )
    }

    pub fn state_label(&self) -> &'static str {
        match self.state {
            SetupTaskStateV1::Detected => "Detected",
            SetupTaskStateV1::ReadyToFix => "Ready to fix",
            SetupTaskStateV1::BlockedByPrerequisite => "Blocked by prerequisite",
            SetupTaskStateV1::NeedsApproval => "Needs approval",
            SetupTaskStateV1::InProgress => "In progress",
            SetupTaskStateV1::AwaitingVerification => "Awaiting verification",
            SetupTaskStateV1::NeedsVerification => "Needs verification",
            SetupTaskStateV1::Verified => "Verified",
            SetupTaskStateV1::Failed => "Failed",
            SetupTaskStateV1::Unsupported => "Unavailable",
            SetupTaskStateV1::Cancelled => "Cancelled",
            SetupTaskStateV1::Paused => "Paused",
            SetupTaskStateV1::Interrupted => "Interrupted",
            SetupTaskStateV1::Superseded => "Replaced by specific checks",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetupTaskStoreV1 {
    pub schema_version: u8,
    pub latest_scan_generation: u64,
    pub tasks: BTreeMap<String, SetupTaskV1>,
}

pub type Store = SetupTaskStoreV1;

impl Default for SetupTaskStoreV1 {
    fn default() -> Self {
        Self {
            schema_version: FILE_SCHEMA_VERSION,
            latest_scan_generation: 0,
            tasks: BTreeMap::new(),
        }
    }
}

pub fn authorization_scope_digest(
    task_id: &str,
    operation_id: &str,
    version: u16,
    scopes: &[String],
) -> String {
    let mut canonical = scopes.to_vec();
    canonical.sort();
    canonical.dedup();
    let binding = format!(
        "setup-attempt-v1\n{task_id}\n{operation_id}\n{version}\n{}",
        canonical.join("\n")
    );
    format!("{:x}", sha2::Sha256::digest(binding.as_bytes()))
}

pub(super) fn positive_domain_for(id: &str, snapshot: &MachineSnapshot) -> Option<String> {
    if id == "setup:legacy:provider" {
        return snapshot
            .checked_domains
            .iter()
            .find(|domain| domain.starts_with("cli:"))
            .cloned();
    }
    let domain = validation::expected_evidence_domain(id)?;
    snapshot
        .checked_domains
        .iter()
        .any(|checked| checked == &domain)
        .then_some(domain)
}

pub(super) fn record(task: &mut SetupTaskV1, at: u64, generation: u64) {
    task.events.push(SetupTaskEventV1 {
        observed_at_unix_ms: at,
        scan_generation: generation,
        state: task.state,
    });
    if task.events.len() > super::MAX_EVENTS {
        task.events.drain(..task.events.len() - super::MAX_EVENTS);
    }
}
