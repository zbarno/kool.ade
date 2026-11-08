use super::types::{InvestigationOutcomeV1, InvestigationReceiptV1, InvestigationRequestV1};
use crate::core::repository_knowledge::{EvidenceRefV1, RepositoryKnowledgeProfileV1};
use sha2::Digest;
use std::collections::BTreeMap;

pub(super) fn evidence_map(
    profile: &RepositoryKnowledgeProfileV1,
) -> Result<BTreeMap<String, EvidenceRefV1>, String> {
    let mut result = BTreeMap::new();
    for reference in &profile.manifests {
        insert_evidence(&mut result, reference)?;
    }
    for claim in profile
        .build_and_test_conventions
        .iter()
        .chain(&profile.top_level_subsystems)
        .chain(&profile.entry_points_and_interfaces)
        .chain(&profile.dependencies)
        .chain(&profile.execution_boundaries)
        .chain(&profile.observed_capabilities)
    {
        for reference in &claim.evidence.references {
            insert_evidence(&mut result, reference)?;
        }
    }
    for candidate in &profile.navigation_candidates {
        let reference = EvidenceRefV1 {
            path: candidate.path.clone(),
            symbol: None,
            line_start: None,
            line_end: None,
            content_sha256: candidate.document_content_sha256.clone(),
            source_sha: candidate.source_sha.clone(),
            kind: crate::core::repository_knowledge::EvidenceKind::Documentation,
        };
        insert_evidence(&mut result, &reference)?;
    }
    Ok(result)
}

fn insert_evidence(
    result: &mut BTreeMap<String, EvidenceRefV1>,
    reference: &EvidenceRefV1,
) -> Result<(), String> {
    if result.get(&reference.path).is_some_and(|prior| {
        prior.content_sha256 != reference.content_sha256 || prior.source_sha != reference.source_sha
    }) {
        return Err("repository profile contains conflicting evidence for one path".into());
    }
    result
        .entry(reference.path.clone())
        .or_insert_with(|| reference.clone());
    Ok(())
}

pub(super) fn stale_receipt(request: &InvestigationRequestV1) -> InvestigationReceiptV1 {
    InvestigationReceiptV1 {
        schema_version: 1,
        idempotency_key: request.idempotency_key.clone(),
        repository_id: request.repository_id.clone(),
        source_sha: request.source_sha.clone(),
        inspected_scope: Vec::new(),
        source_refs: Vec::new(),
        comparison_refs: Vec::new(),
        findings: Vec::new(),
        uncertainty: vec![
            "The selected source or planning revision changed before investigation.".into(),
        ],
        omitted_scope: request.scope.paths.clone(),
        files_read: 0,
        bytes_read: 0,
        elapsed_ms: 0,
        confidence: 0,
        outcome: InvestigationOutcomeV1::StaleSource,
    }
}

pub(super) fn safe_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let sensitive_keys = [
        "password",
        "passwd",
        "secret",
        "token",
        "credential",
        "api_key",
        "apikey",
        "private_key",
        "private-key",
        "authorization",
        "access_key",
        "client_secret",
    ];
    if sensitive_keys.iter().any(|key| lower.contains(key))
        || [
            "ghp_",
            "github_pat_",
            "sk_live_",
            "sk_test_",
            "-----begin ",
            "bearer ",
            "akia",
            "xox",
            "gho_",
            "ghu_",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return "[sensitive repository content omitted]".into();
    }
    let mut result = String::new();
    for token in line.split_whitespace() {
        let lower_token = token.to_ascii_lowercase();
        if [
            "http://",
            "https://",
            "ssh://",
            "tcp://",
            "postgres://",
            "redis://",
        ]
        .iter()
        .any(|scheme| lower_token.contains(scheme))
            || [
                "localhost",
                ".local",
                ".corp",
                ".internal",
                ".private",
                ".svc",
                "127.0.0.1",
                "10.",
                "172.16.",
                "192.168.",
            ]
            .iter()
            .any(|marker| lower_token.contains(marker))
        {
            result.push_str("[URL omitted] ");
        } else if token.contains("/home/") || token.contains("/mnt/") || token.contains("/Users/") {
            result.push_str("[host path omitted] ");
        } else if token.len() >= 24
            && token.bytes().any(|byte| byte.is_ascii_digit())
            && token.bytes().any(|byte| byte.is_ascii_alphabetic())
            && token.bytes().any(|byte| byte.is_ascii_lowercase())
            && token.bytes().any(|byte| byte.is_ascii_uppercase())
        {
            result.push_str("[long identifier omitted] ");
        } else {
            result.push_str(token);
            result.push(' ');
        }
    }
    result
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(320)
        .collect()
}

pub(super) fn fingerprint(request: &InvestigationRequestV1) -> Result<String, String> {
    let bytes = serde_json::to_vec(request)
        .map_err(|_| "investigation request could not be fingerprinted")?;
    Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}
