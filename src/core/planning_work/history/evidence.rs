use serde::{Deserialize, Serialize};

use crate::core::lifecycle::{
    CurrentImplementationEvidenceV1, DeploymentEvidenceV1, GateReceipt, TaskPhase,
};

/// Evidence copied from authoritative planning and lifecycle records.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "evidence", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskIntentEventEvidenceV1 {
    Decision {
        decision_uid: Option<String>,
        status: crate::domain::ItemStatus,
        subject_sha256: String,
    },
    PlanDecision {
        feature_uid: String,
        snapshot_uid: String,
        snapshot_sha256: String,
        receipt: GateReceipt,
    },
    GateReceipt {
        task_uid: String,
        gate: TaskIntentGateKindV1,
        receipt: GateReceipt,
    },
    TaskOutcome {
        task_uid: String,
        phase: Option<TaskPhase>,
        status: Option<crate::core::planning_work::WorkStatus>,
        pull_request: Option<crate::artifacts::task_docs::PullRequestLinkage>,
    },
    Deployment {
        deployment: DeploymentEvidenceV1,
        current_implementation: CurrentImplementationEvidenceV1,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskIntentGateKindV1 {
    IndependentReview,
    RuntimeQa,
    SpecificationReconciliation,
}

impl TaskIntentEventEvidenceV1 {
    pub(crate) fn validate(&self, related_uid: Option<&str>) -> bool {
        match self {
            Self::Decision {
                decision_uid,
                subject_sha256,
                ..
            } => {
                decision_uid.as_deref().is_none_or(valid_uuid)
                    && valid_sha256(subject_sha256)
                    && decision_uid
                        .as_deref()
                        .is_none_or(|uid| related_uid == Some(uid))
            }
            Self::PlanDecision {
                feature_uid,
                snapshot_uid,
                snapshot_sha256,
                receipt,
            } => {
                valid_uuid(feature_uid)
                    && valid_uuid(snapshot_uid)
                    && valid_sha256(snapshot_sha256)
                    && receipt.validate().is_ok()
                    && receipt.actor == crate::core::lifecycle::GateActor::User
                    && receipt.decision != crate::core::lifecycle::GateDecision::NotApplicable
                    && receipt.subject_uid == *snapshot_uid
                    && receipt.subject_sha256 == *snapshot_sha256
                    && related_uid == Some(snapshot_uid)
            }
            Self::GateReceipt {
                task_uid, receipt, ..
            } => {
                valid_uuid(task_uid)
                    && related_uid == Some(task_uid)
                    && receipt.validate().is_ok()
                    && receipt.actor != crate::core::lifecycle::GateActor::User
                    && receipt.subject_uid == *task_uid
            }
            Self::TaskOutcome { task_uid, .. } => {
                valid_uuid(task_uid) && related_uid == Some(task_uid)
            }
            Self::Deployment {
                deployment,
                current_implementation,
            } => {
                deployment.feature_uid == current_implementation.feature_uid
                    && deployment.is_successful_production_for_current(current_implementation)
                    && related_uid == Some(current_implementation.uid.as_str())
            }
        }
    }
}

fn valid_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
