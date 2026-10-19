use sha2::Digest;

use super::{KOOLADE_PRODUCT_TARGET_REF, ManagerPrompt, opportunities};
use crate::harness::prompt_catalog::{
    OutputValidationContext, PromptRole, RoleInputV1, RolePayloadV1,
};

/// Product proposals use only public product evidence and an empty app-owned cwd.
pub fn render_product(
    project: &crate::app::session::Project,
) -> Result<Option<ManagerPrompt>, String> {
    let Some(repository) = project
        .state
        .repositories
        .repositories
        .iter()
        .find(|repository| is_koolade_product_remote(&repository.remote))
    else {
        return Ok(None);
    };
    let workspace_guard = tempfile::tempdir().map_err(|error| error.to_string())?;
    let repo_root = workspace_guard.path().to_path_buf();
    let scan_uid = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        b"https://github.com/zbarno/kool.ade/public-opportunity-scan",
    )
    .hyphenated()
    .to_string();
    let evidence = vec![
        opportunities::koolade_product_capability_evidence(&scan_uid),
        opportunities::koolade_product_contract_evidence(&scan_uid),
    ];
    let evidence_ids = evidence
        .iter()
        .map(|item| item.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let evidence_hashes = evidence
        .iter()
        .map(|item| {
            (
                item.id.clone(),
                format!("{:x}", sha2::Sha256::digest(item.content.as_bytes())),
            )
        })
        .collect();
    let evidence_confidentialities = evidence
        .iter()
        .map(|item| (item.id.clone(), item.confidentiality))
        .collect();
    let input = RoleInputV1 {
        role: PromptRole::Manager,
        workspace_uid: scan_uid.clone(),
        feature_uid: scan_uid,
        task_uid: None,
        original_goal: format!(
            "PRODUCT-ONLY OPPORTUNITY REVIEW: identify a concrete improvement using only the public Kool.ad/e product contract and capability inventory supplied below. The exact target is target_repository_ref={KOOLADE_PRODUCT_TARGET_REF}, target_repository={}. Use those values unchanged. Do not use or request information from a linked user project.",
            repository.remote
        ),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: Vec::new(),
        evidence_manifest: evidence,
        execution_policy_ref: "manager-policy/public-product-opportunity/v1".into(),
        output_schema_version: PromptRole::Manager.output_schema_version().into(),
        payload: RolePayloadV1::Manager {
            board_event_cursor: "public-product-opportunity-scan".into(),
            work_graph: Vec::new(),
            unresolved_findings: Vec::new(),
            latest_review_receipts: Vec::new(),
        },
    };
    let allowed_repositories = std::collections::HashMap::from([(
        KOOLADE_PRODUCT_TARGET_REF.to_owned(),
        repository.remote.clone(),
    )]);
    let evidence_repository_refs = evidence_ids
        .iter()
        .map(|id| (id.clone(), KOOLADE_PRODUCT_TARGET_REF.to_owned()))
        .collect();
    Ok(Some(ManagerPrompt {
        rendered: crate::harness::prompt_catalog::render(&input)?,
        validation: OutputValidationContext {
            evidence_ids: evidence_ids.clone(),
            public_evidence_ids: evidence_ids,
            allowed_repositories,
            koolade_product_repository_ref: Some(KOOLADE_PRODUCT_TARGET_REF.to_owned()),
            evidence_repository_refs,
            ..Default::default()
        },
        evidence_hashes,
        evidence_confidentialities,
        repo_root: Some(repo_root),
        _workspace_guard: Some(workspace_guard),
    }))
}

pub(super) fn is_koolade_product_remote(remote: &str) -> bool {
    let remote = remote.trim().trim_end_matches(".git").to_ascii_lowercase();
    matches!(
        remote.as_str(),
        "https://github.com/zbarno/kool.ade"
            | "http://github.com/zbarno/kool.ade"
            | "git@github.com:zbarno/kool.ade"
            | "ssh://git@github.com/zbarno/kool.ade"
    )
}
