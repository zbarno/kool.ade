use serde::{Deserialize, Serialize};

const CATALOG: &str = include_str!("../../../assets/capabilities/product-capabilities.json");
const MAX_CATALOG_BYTES: usize = 128 * 1024;
const MAX_ENTRIES: usize = 128;
const MAX_RESULTS: usize = 5;
mod validation;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductCapabilityCatalogV1 {
    pub schema_version: u16,
    pub capabilities: Vec<Capability>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub maturity: String,
    pub terms: Vec<String>,
    pub status: CapabilityStatus,
    pub invocation: Vec<String>,
    pub constraints: Vec<String>,
    pub platform_prerequisites: Vec<String>,
    pub provider_prerequisites: Vec<String>,
    pub limitations: Vec<String>,
    pub implementation_refs: Vec<String>,
    pub test_refs: Vec<String>,
    pub readiness_probe_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Implemented,
    Planned,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityAnswer {
    pub build_version: Option<String>,
    pub build_commit: Option<String>,
    pub catalog_sha256: String,
    pub matches: Vec<CapabilityMatch>,
    pub confidence: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityMatch {
    pub capability: Capability,
    pub state: CapabilityState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityState {
    ImplementedInBuild,
    VerifiedReady,
    ImplementedButNotReady,
    Planned,
    Unsupported,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadinessState {
    Ready,
    NotReady,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessEvidence {
    capability_id: String,
    readiness_probe_id: String,
    build_commit: String,
    probe_evidence_sha256: String,
    state: ReadinessState,
}

impl ReadinessEvidence {
    pub(crate) fn from_registered_probe(
        capability_id: String,
        readiness_probe_id: String,
        build_commit: String,
        probe_evidence_sha256: String,
    ) -> Result<Self, String> {
        if !valid_full_sha(&build_commit) || !valid_full_sha(&probe_evidence_sha256) {
            return Err("readiness evidence is missing a valid build or probe digest".into());
        }
        Ok(Self {
            capability_id,
            readiness_probe_id,
            build_commit,
            probe_evidence_sha256,
            state: ReadinessState::Ready,
        })
    }
}

impl ProductCapabilityCatalogV1 {
    pub fn compiled() -> Result<Self, String> {
        if CATALOG.len() > MAX_CATALOG_BYTES {
            return Err("product capability catalog exceeds size limit".into());
        }
        let value: Self = serde_json::from_str(CATALOG).map_err(|error| error.to_string())?;
        validation::validate(&value)?;
        Ok(value)
    }

    pub fn lookup(&self, query: &str) -> CapabilityAnswer {
        self.lookup_with_readiness(query, &[])
    }

    pub fn lookup_with_readiness(
        &self,
        query: &str,
        evidence: &[ReadinessEvidence],
    ) -> CapabilityAnswer {
        use sha2::{Digest, Sha256};
        let (build_version, build_commit) = crate::build_identity::current()
            .map(|(version, commit)| (Some(version.to_owned()), Some(commit.to_owned())))
            .unwrap_or((None, None));
        let query_tokens = tokens(query);
        let mut ranked = self
            .capabilities
            .iter()
            .filter_map(|item| {
                let terms = std::iter::once(item.name.as_str())
                    .chain(std::iter::once(item.summary.as_str()))
                    .chain(item.terms.iter().map(String::as_str));
                let score = terms
                    .map(|term| overlap(&query_tokens, term))
                    .max()
                    .unwrap_or_default();
                (score > 0).then_some((score, item))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| left.id.cmp(&right.id))
        });
        let matches = ranked
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, item)| CapabilityMatch {
                capability: item.clone(),
                state: match item.status {
                    CapabilityStatus::Implemented => {
                        if build_commit.is_some() {
                            if item.readiness_probe_id.is_some() {
                                readiness_state(item, build_commit.as_deref(), evidence)
                            } else {
                                CapabilityState::ImplementedInBuild
                            }
                        } else {
                            CapabilityState::Unknown
                        }
                    }
                    CapabilityStatus::Planned => CapabilityState::Planned,
                    CapabilityStatus::Unsupported => CapabilityState::Unsupported,
                },
            })
            .collect::<Vec<_>>();
        CapabilityAnswer {
            build_version,
            build_commit,
            catalog_sha256: format!("{:x}", Sha256::digest(CATALOG.as_bytes())),
            confidence: if matches.is_empty() {
                "unknown"
            } else {
                "candidate_match"
            },
            matches,
        }
    }

    pub fn public_snapshot() -> String {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Snapshot<'a> {
            build_version: Option<String>,
            build_commit: Option<String>,
            catalog_sha256: String,
            catalog_status: &'static str,
            catalog: Option<&'a ProductCapabilityCatalogV1>,
        }
        use sha2::{Digest, Sha256};
        let (build_version, build_commit) = crate::build_identity::current()
            .map(|(version, commit)| (Some(version.to_owned()), Some(commit.to_owned())))
            .unwrap_or((None, None));
        let catalog = Self::compiled().ok();
        serde_json::to_string(&Snapshot {
            build_version,
            build_commit,
            catalog_sha256: format!("{:x}", Sha256::digest(CATALOG.as_bytes())),
            catalog_status: if catalog.is_some() {
                "validated"
            } else {
                "unknown"
            },
            catalog: catalog.as_ref(),
        })
        .unwrap_or_else(|_| "{\"confidence\":\"unknown\"}".into())
    }
}

fn readiness_state(
    capability: &Capability,
    build_commit: Option<&str>,
    evidence: &[ReadinessEvidence],
) -> CapabilityState {
    let (Some(build_commit), Some(readiness_probe_id)) =
        (build_commit, capability.readiness_probe_id.as_deref())
    else {
        return CapabilityState::Unknown;
    };
    let matching = evidence
        .iter()
        .filter(|item| {
            item.capability_id == capability.id
                && item.readiness_probe_id == readiness_probe_id
                && item.build_commit == build_commit
                && valid_full_sha(&item.probe_evidence_sha256)
        })
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return CapabilityState::ImplementedInBuild;
    }
    if matching.len() != 1 {
        return CapabilityState::Unknown;
    }
    match matching[0].state {
        ReadinessState::Ready => CapabilityState::VerifiedReady,
        ReadinessState::NotReady => CapabilityState::ImplementedButNotReady,
        ReadinessState::Unavailable => CapabilityState::Unknown,
    }
}

fn valid_full_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn overlap(query: &[String], term: &str) -> usize {
    let term = tokens(term);
    query.iter().filter(|word| term.contains(*word)).count()
}

fn tokens(value: &str) -> Vec<String> {
    value
        .chars()
        .take(4096)
        .collect::<String>()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 3)
        .take(64)
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
