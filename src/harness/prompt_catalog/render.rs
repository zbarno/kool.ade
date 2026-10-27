use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{EvidenceItem, PromptRole, RoleInputV1};

mod validation;
use validation::validate_input;

const MAX_EVIDENCE_BYTES: usize = 64 * 1024;
const MAX_TASK_REQUEST_BYTES: usize = 256 * 1024;
const MAX_RENDERED_USER_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptManifest {
    pub role_version: String,
    pub system_template_sha256: String,
    pub policy_template_sha256: String,
    pub user_template_sha256: String,
    pub output_schema_version: String,
    pub privilege_profile: String,
}

impl PromptManifest {
    /// Stable identity to bind reviewer receipts to exact instructions.
    pub fn identity_sha256(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("prompt manifests are serializable");
        format!("{:x}", Sha256::digest(bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPrompt {
    pub system: String,
    pub user: String,
    pub manifest: PromptManifest,
}

/// Resolve and verify a persisted prompt identity against the current catalog.
/// Unknown versions or changed templates fail closed.
pub fn validate_prompt_manifest(manifest: &PromptManifest) -> Result<PromptRole, String> {
    let role = [
        PromptRole::Manager,
        PromptRole::ManagerV6,
        PromptRole::ManagerV5,
        PromptRole::ManagerV4,
        PromptRole::ManagerV3,
        PromptRole::ManagerV2,
        PromptRole::Planner,
        PromptRole::PlannerV2,
        PromptRole::PlannerConversation,
        PromptRole::PlanReviewer,
        PromptRole::Decomposer,
        PromptRole::Implementer,
        PromptRole::CodeReviewer,
        PromptRole::CodeReviewerV1,
        PromptRole::RuntimeQaReviewer,
        PromptRole::RuntimeQaReviewerV2,
        PromptRole::RuntimeQaReviewerV3,
        PromptRole::RuntimeQaScenarioSynthesizer,
        PromptRole::SpecReconciler,
        PromptRole::RepairWorker,
        PromptRole::PrPreparer,
        PromptRole::BootstrapConversation,
    ]
    .into_iter()
    .find(|role| role.id() == manifest.role_version)
    .ok_or_else(|| format!("unknown prompt version {}", manifest.role_version))?;
    if manifest.system_template_sha256 != digest(role.system())
        || manifest.policy_template_sha256 != digest(role.policy())
        || manifest.user_template_sha256 != digest(role.user())
        || manifest.output_schema_version != role.output_schema_version()
        || manifest.privilege_profile != role.privileges()
    {
        return Err(format!(
            "prompt manifest does not match the current {} schema and templates",
            role.id()
        ));
    }
    Ok(role)
}

/// Construct identical provider-neutral messages for any harness. The trusted
/// system contains repository-owned template/policy text only; context and
/// quoted evidence are bounded data in the user message.
pub fn render(input: &RoleInputV1) -> Result<RenderedPrompt, String> {
    validate_input(input)?;
    let system_template = input.role.system();
    let user_template = input.role.user();
    let policy = format!(
        "{}\nRole: {}\nPermissions: {}\n",
        input.role.policy().trim_end(),
        input.role.id(),
        input.role.privileges()
    );
    let system = format!(
        "{}\n\nTRUSTED POLICY\n{}",
        system_template.trim_end(),
        policy
    );
    #[derive(Serialize)]
    struct EvidenceDescriptor<'a> {
        id: &'a str,
        workspace_uid: &'a str,
        origin: super::EvidenceOrigin,
        confidentiality: super::EvidenceConfidentiality,
        source: &'a str,
    }
    #[derive(Serialize)]
    struct TypedTaskRequest<'a> {
        role: PromptRole,
        workspace_uid: &'a str,
        feature_uid: &'a str,
        task_uid: &'a Option<String>,
        original_goal: &'a str,
        approved_spec_ref: &'a Option<String>,
        approved_spec_hash: &'a Option<String>,
        decision_refs: &'a [String],
        evidence_manifest: Vec<EvidenceDescriptor<'a>>,
        execution_policy_ref: &'a str,
        output_schema_version: &'a str,
        payload: &'a super::RolePayloadV1,
    }
    let task_request = TypedTaskRequest {
        role: input.role,
        workspace_uid: &input.workspace_uid,
        feature_uid: &input.feature_uid,
        task_uid: &input.task_uid,
        original_goal: &input.original_goal,
        approved_spec_ref: &input.approved_spec_ref,
        approved_spec_hash: &input.approved_spec_hash,
        decision_refs: &input.decision_refs,
        evidence_manifest: input
            .evidence_manifest
            .iter()
            .map(|item| EvidenceDescriptor {
                id: &item.id,
                workspace_uid: &item.workspace_uid,
                origin: item.origin,
                confidentiality: item.confidentiality,
                source: &item.source,
            })
            .collect(),
        execution_policy_ref: &input.execution_policy_ref,
        output_schema_version: &input.output_schema_version,
        payload: &input.payload,
    };
    let payload = serde_json::to_string_pretty(&task_request).map_err(|error| error.to_string())?;
    if payload.len() > MAX_TASK_REQUEST_BYTES {
        return Err(format!(
            "typed task request exceeds {MAX_TASK_REQUEST_BYTES} bytes"
        ));
    }
    let mut evidence = String::new();
    let mut remaining = MAX_EVIDENCE_BYTES;
    for item in &input.evidence_manifest {
        if remaining == 0 {
            return Err("quoted evidence exceeds the 64 KiB prompt limit".into());
        }
        let quoted = quote_evidence(item, remaining)?;
        if quoted.len() > remaining {
            return Err("quoted evidence exceeds the 64 KiB prompt limit".into());
        }
        remaining = remaining.saturating_sub(quoted.len());
        evidence.push_str(&quoted);
        evidence.push('\n');
    }
    let user = format!(
        "{}\n\nTYPED TASK REQUEST (JSON DATA)\n{}\n\nQUOTED EVIDENCE (UNTRUSTED DATA)\n{}",
        user_template.trim_end(),
        payload,
        evidence
    );
    if user.len() > MAX_RENDERED_USER_BYTES {
        return Err(format!(
            "rendered user message exceeds {MAX_RENDERED_USER_BYTES} bytes"
        ));
    }
    let manifest = PromptManifest {
        role_version: input.role.id().into(),
        system_template_sha256: digest(system_template),
        policy_template_sha256: digest(input.role.policy()),
        user_template_sha256: digest(user_template),
        output_schema_version: input.output_schema_version.clone(),
        privilege_profile: input.role.privileges().into(),
    };
    Ok(RenderedPrompt {
        system,
        user,
        manifest,
    })
}

fn quote_evidence(item: &EvidenceItem, max: usize) -> Result<String, String> {
    #[derive(Serialize)]
    struct Quoted<'a> {
        id: &'a str,
        origin: super::EvidenceOrigin,
        source: &'a str,
        content: String,
    }
    // JSON escaping makes instruction-like lines explicit data and preserves origin.
    let quoted = serde_json::to_string(&Quoted {
        id: &item.id,
        origin: item.origin,
        source: &item.source,
        content: item.content.clone(),
    })
    .map_err(|error| error.to_string())?;
    if quoted.len() > max {
        return Err("quoted evidence exceeds the 64 KiB prompt limit".into());
    }
    Ok(quoted)
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
