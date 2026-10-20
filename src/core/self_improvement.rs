//! Tracks Kool.ad/e product improvements through the ordinary task lifecycle.
//! This record tracks PR/activation; it grants no execution or publication authority.

use serde::{Deserialize, Serialize};

mod capability_probe;
mod run_validation;
mod store;
mod transition;
mod validation;
pub use store::{SelfImprovementStore, SelfImprovementTransitionV1, TaskSourceAuthorization};
pub(crate) use validation::canonical_product_remote;

pub const MAX_SELF_IMPROVEMENT_CHAIN_DEPTH: u8 = 2;

#[cfg(test)]
#[path = "self_improvement/tests.rs"]
mod tests;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfImprovementEventV1 {
    BeginPlanning,
    PlanApproved {
        approval_sha256: String,
        #[serde(default)]
        capability_id: Option<String>,
        review_uid: String,
        source_ref: String,
        destination_ref: String,
        task_uids: Vec<String>,
        owner_task_uid: String,
        #[serde(default)]
        parent_task_uids: Vec<String>,
    },
    ParentTaskProgress(SelfImprovementParentTaskProgressV1),
    CloneAllocated {
        clone_uid: String,
    },
    ImplementationFinished {
        head_sha: String,
        subject_sha256: String,
    },
    CodeReviewPassed {
        receipt_uid: String,
        subject_sha256: String,
    },
    QaPassed {
        receipt_uid: String,
        subject_sha256: String,
    },
    SpecReconciled {
        receipt_uid: String,
        subject_sha256: String,
    },
    PrPrepared {
        work_item_uid: String,
    },
    PrOpened {
        url: String,
    },
    PrClosedUnmerged {
        url: String,
    },
    PrReopened,
    Merged {
        sha: String,
    },
    Installed {
        version: String,
        build_commit_sha: String,
        operator_id: String,
    },
    Activated {
        evidence: SelfImprovementActivationEvidenceV1,
    },
    NeedsAttention,
    Cancel,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfImprovementPhaseV1 {
    ApprovedToPlan,
    Planning,
    Ready,
    Implementing,
    Reviewing,
    Qa,
    SpecSync,
    DoneAwaitingPr,
    PrPending,
    PrOpened,
    PrClosedUnmerged,
    MergedNotActivated,
    InstalledNotActivated,
    Activated,
    NeedsAttention,
    Cancelled,
}

/// Projection of a regular Kool.ad/e feature/task run. Plan, code review,
/// runtime QA, spec sync and publication still use their existing app-owned gates.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelfImprovementRunV1 {
    pub schema_version: u16,
    pub run_uid: String,
    pub opportunity_uid: String,
    pub approved_decision_uid: String,
    pub target_repository_uid: String,
    pub canonical_remote: String,
    pub source_ref: Option<String>,
    pub source_sha: String,
    pub destination_ref: Option<String>,
    pub feature_uid: String,
    /// Parent improvement when this run was explicitly pursued from related
    /// self-improvement work. The depth is durable and capped across restarts.
    #[serde(default)]
    pub parent_run_uid: Option<String>,
    #[serde(default)]
    pub chain_depth: u8,
    /// Product capability declared in the approved plan snapshot.
    #[serde(default)]
    pub capability_id: Option<String>,
    pub task_uids: Vec<String>,
    pub owner_task_uid: Option<String>,
    /// Durable progress for every standalone parent task. Older run records
    /// are read with an empty list and are backfilled by the app synchronizer.
    #[serde(default)]
    pub parent_task_progress: Vec<SelfImprovementParentTaskProgressV1>,
    pub clone_uid: Option<String>,
    pub plan_approval_hash: Option<String>,
    pub independent_plan_review_uid: Option<String>,
    pub implementation_head_sha: Option<String>,
    pub implementation_subject_sha256: Option<String>,
    pub code_review_uids: Vec<String>,
    pub runtime_qa_uids: Vec<String>,
    pub spec_receipt_uid: Option<String>,
    pub pr_work_item_uid: Option<String>,
    pub pr_url: Option<String>,
    pub merged_sha: Option<String>,
    #[serde(default)]
    pub installed_version: Option<String>,
    #[serde(default)]
    pub installed_build_commit_sha: Option<String>,
    pub activated_version: Option<String>,
    pub phase: SelfImprovementPhaseV1,
    pub revision: u64,
}

/// Evidence projection for one standalone task and its separately published
/// PR. Child subtasks remain represented by their parent lifecycle record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelfImprovementParentTaskProgressV1 {
    pub task_uid: String,
    pub phase: crate::core::lifecycle::TaskPhase,
    pub implementation_sha256: Option<String>,
    pub code_review_uid: Option<String>,
    pub runtime_qa_uid: Option<String>,
    pub spec_receipt_uid: Option<String>,
    pub pr_work_item_uid: Option<String>,
    pub pr_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
    pub pr_url: Option<String>,
    pub pr_head_sha: Option<String>,
    #[serde(default)]
    pub pr_base_branch: Option<String>,
    #[serde(default)]
    pub pr_merged_commit_sha: Option<String>,
    #[serde(default)]
    pub pr_merged_at: Option<String>,
}

/// App-owned evidence produced only after probing the active installed build.
/// The merge SHA binding prevents readiness for an unrelated build from
/// activating this run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelfImprovementActivationEvidenceV1 {
    feature_uid: String,
    capability_id: String,
    readiness_probe_id: String,
    capability_manifest_sha256: String,
    version: String,
    merged_sha: String,
    build_commit_sha: String,
    readiness_probe_uid: String,
}

impl SelfImprovementActivationEvidenceV1 {
    pub(crate) fn probe_active_build(run: &SelfImprovementRunV1) -> Result<Self, String> {
        run.validate()?;
        if run.phase != SelfImprovementPhaseV1::InstalledNotActivated {
            return Err("active capability probe requires an installed, inactive run".into());
        }
        let (version, build_commit_sha) = crate::build_identity::current()
            .ok_or_else(|| "active application build identity is unavailable".to_owned())?;
        if run.installed_version.as_deref() != Some(version)
            || run.installed_build_commit_sha.as_deref() != Some(build_commit_sha)
        {
            return Err("active application build differs from the recorded installation".into());
        }
        let merged_sha = run
            .merged_sha
            .as_deref()
            .ok_or_else(|| "merged self-improvement commit is unavailable".to_owned())?;
        let capability = capability_probe::probe_compiled_capability(run)?;
        activation_evidence(
            &run.run_uid,
            &run.feature_uid,
            &capability,
            merged_sha,
            version,
            build_commit_sha,
        )
    }
}

fn activation_evidence(
    run_uid: &str,
    feature_uid: &str,
    capability: &capability_probe::ActiveCapabilityProbeEvidence,
    merged_sha: &str,
    version: &str,
    build_commit_sha: &str,
) -> Result<SelfImprovementActivationEvidenceV1, String> {
    if !crate::core::self_improvement::validation::is_uuid(run_uid)
        || !crate::core::self_improvement::validation::is_uuid(feature_uid)
        || !crate::core::self_improvement::validation::is_git_sha(merged_sha)
        || merged_sha != build_commit_sha
        || !crate::core::self_improvement::validation::is_git_sha(build_commit_sha)
        || !crate::core::self_improvement::validation::valid_text(version, 128)
        || !crate::core::self_improvement::validation::is_digest(&capability.manifest_sha256)
    {
        return Err("active capability probe did not match the merged build".into());
    }
    let namespace = uuid::Uuid::parse_str(run_uid).map_err(|error| error.to_string())?;
    let readiness_probe_uid = uuid::Uuid::new_v5(
        &namespace,
        format!(
            "active-capability:{}:{}:{}:{}:{}",
            feature_uid,
            capability.capability_id,
            capability.readiness_probe_id,
            version,
            capability.manifest_sha256
        )
        .as_bytes(),
    )
    .hyphenated()
    .to_string();
    Ok(SelfImprovementActivationEvidenceV1 {
        feature_uid: feature_uid.to_owned(),
        capability_id: capability.capability_id.clone(),
        readiness_probe_id: capability.readiness_probe_id.clone(),
        capability_manifest_sha256: capability.manifest_sha256.clone(),
        version: version.to_owned(),
        merged_sha: merged_sha.to_owned(),
        build_commit_sha: build_commit_sha.to_owned(),
        readiness_probe_uid,
    })
}

#[cfg(test)]
#[path = "self_improvement/activation_tests.rs"]
mod activation_tests;

#[cfg(test)]
#[path = "self_improvement/acceptance_tests.rs"]
mod acceptance_tests;
