use serde::{Deserialize, Serialize};

const MAX_TEXT_CHARS: usize = 2_000;
const MAX_LIST_ITEMS: usize = 128;

/// Cache identity deliberately includes workspace-local repository identity,
/// selected ref, immutable source commit, and whether its checkout is dirty.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositoryRevisionKeyV1 {
    pub repository_id: String,
    pub source_ref: String,
    pub commit_sha: String,
    pub dirty_checkout: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeProfileStatus {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Verified,
    SupportedBySource,
    Inferred,
    Unknown,
    Stale,
    Contradicted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Manifest,
    Source,
    Test,
    Documentation,
    Navigation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceRefV1 {
    pub path: String,
    pub symbol: Option<String>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub content_sha256: String,
    pub source_sha: String,
    pub kind: EvidenceKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClaimEvidenceV1 {
    pub status: ClaimStatus,
    pub confidence: u8,
    pub references: Vec<EvidenceRefV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArchitectureClaimV1 {
    pub id: String,
    pub area: String,
    pub statement: String,
    pub evidence: ClaimEvidenceV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScanOmissionV1 {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NavigationCandidateV1 {
    pub path: String,
    pub purpose: String,
    pub index_path: String,
    pub index_content_sha256: String,
    pub document_content_sha256: String,
    pub source_sha: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScanStatsV1 {
    pub tree_entries_seen: u32,
    pub files_read: u32,
    pub bytes_read: u64,
    pub navigation_indexes_read: u16,
    pub documents_read: u16,
    pub truncated: bool,
}

/// Bounded summary and evidence index. File contents are retrieved separately
/// at the exact source SHA and are never cached as a prompt-sized repository dump.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositoryKnowledgeProfileV1 {
    pub schema_version: u32,
    pub key: RepositoryRevisionKeyV1,
    pub scanned_at_unix_ms: u64,
    pub status: KnowledgeProfileStatus,
    pub languages: Vec<String>,
    pub manifests: Vec<EvidenceRefV1>,
    pub build_and_test_conventions: Vec<ArchitectureClaimV1>,
    pub top_level_subsystems: Vec<ArchitectureClaimV1>,
    pub entry_points_and_interfaces: Vec<ArchitectureClaimV1>,
    pub dependencies: Vec<ArchitectureClaimV1>,
    pub execution_boundaries: Vec<ArchitectureClaimV1>,
    pub observed_capabilities: Vec<ArchitectureClaimV1>,
    pub navigation_candidates: Vec<NavigationCandidateV1>,
    pub omissions: Vec<ScanOmissionV1>,
    pub scan: ScanStatsV1,
}

impl RepositoryKnowledgeProfileV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.key.repository_id.is_empty()
            || self.key.repository_id.len() > 128
            || self.key.source_ref.is_empty()
            || self.key.source_ref.len() > 256
            || !valid_sha(&self.key.commit_sha)
            || self.scanned_at_unix_ms == 0
            || self.languages.len() > MAX_LIST_ITEMS
            || self.manifests.len() > MAX_LIST_ITEMS
            || self.navigation_candidates.len() > MAX_LIST_ITEMS
            || self.omissions.len() > MAX_LIST_ITEMS
        {
            return Err("repository knowledge profile has an invalid identity or size".into());
        }
        for claim in self.claims() {
            if claim.id.is_empty()
                || claim.id.len() > 128
                || claim.area.is_empty()
                || claim.statement.is_empty()
                || claim.statement.chars().count() > MAX_TEXT_CHARS
                || claim.evidence.confidence > 100
                || claim.evidence.references.is_empty()
                || claim.evidence.references.len() > 16
            {
                return Err("repository knowledge profile contains an invalid claim".into());
            }
            for reference in &claim.evidence.references {
                if !valid_path(&reference.path)
                    || !valid_sha(&reference.source_sha)
                    || reference.source_sha != self.key.commit_sha
                    || !valid_sha(&reference.content_sha256)
                    || reference
                        .line_start
                        .zip(reference.line_end)
                        .is_some_and(|(start, end)| start == 0 || end < start)
                {
                    return Err("repository knowledge profile has invalid provenance".into());
                }
            }
        }
        for reference in &self.manifests {
            if !valid_path(&reference.path)
                || reference.source_sha != self.key.commit_sha
                || !valid_sha(&reference.source_sha)
                || !valid_sha(&reference.content_sha256)
            {
                return Err("repository knowledge profile has invalid manifest provenance".into());
            }
        }
        for candidate in &self.navigation_candidates {
            if !valid_path(&candidate.path)
                || !valid_path(&candidate.index_path)
                || candidate.purpose.is_empty()
                || candidate.purpose.chars().count() > MAX_TEXT_CHARS
                || candidate.source_sha != self.key.commit_sha
                || !valid_sha(&candidate.source_sha)
                || !valid_sha(&candidate.index_content_sha256)
                || !valid_sha(&candidate.document_content_sha256)
            {
                return Err(
                    "repository knowledge profile has invalid navigation provenance".into(),
                );
            }
        }
        Ok(())
    }

    fn claims(&self) -> impl Iterator<Item = &ArchitectureClaimV1> {
        self.build_and_test_conventions
            .iter()
            .chain(&self.top_level_subsystems)
            .chain(&self.entry_points_and_interfaces)
            .chain(&self.dependencies)
            .chain(&self.execution_boundaries)
            .chain(&self.observed_capabilities)
    }
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
