//! Immutable application binding for one Specification conversation turn.

use crate::core::{published_specs::PublishedSpecPin, state::PlannerState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod evidence;
pub(crate) use evidence::validate_reviewed_repository_evidence;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Ask,
    Propose,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Scope {
    Product,
    Feature { display_id: String, uid: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub scope: Scope,
    pub published_pin: PublishedSpecPin,
    /// Exact source-document hashes from the pinned published Git tree.
    pub source_documents: Vec<SourceDocument>,
    /// Also binds an unpublished local proposal when the selected feature has one.
    pub effective_document_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocument {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub text: String,
    pub intent: Intent,
    pub target: Target,
    #[serde(default)]
    pub omitted_history_messages: usize,
    #[serde(default)]
    pub reviewed_repository_evidence: Vec<crate::domain::chatlog::VerifiedRepositoryEvidence>,
}

impl Request {
    pub fn conversation_context(&self) -> crate::domain::chatlog::SpecificationChatContext {
        use crate::domain::chatlog::{SpecificationChatContext, SpecificationSourceDocument};
        let scope_key = match &self.target.scope {
            Scope::Product => "product".to_owned(),
            Scope::Feature { uid, .. } => format!("feature:{uid}"),
        };
        SpecificationChatContext {
            scope_key,
            intent: match self.intent {
                Intent::Ask => "ask".into(),
                Intent::Propose => "propose".into(),
            },
            published_reference: self.target.published_pin.reference.clone(),
            published_commit_sha: self.target.published_pin.commit_sha.clone(),
            source_documents: self
                .target
                .source_documents
                .iter()
                .map(|doc| SpecificationSourceDocument {
                    path: doc.path.clone(),
                    sha256: doc.sha256.clone(),
                })
                .collect(),
            effective_document_sha256: self.target.effective_document_sha256.clone(),
            reviewed_repository_evidence: self
                .reviewed_repository_evidence
                .iter()
                .map(
                    |evidence| crate::domain::chatlog::ReviewedRepositoryEvidenceRef {
                        evidence_id: evidence.evidence_id.clone(),
                        repository_id: evidence.repository_id.clone(),
                        source_ref: evidence.source_ref.clone(),
                        source_sha: evidence.source_sha.clone(),
                        content_sha256: evidence.content_sha256.clone(),
                        excerpt_sha256: evidence.excerpt_sha256.clone(),
                    },
                )
                .collect(),
        }
    }
}

pub fn capture_target(state: &PlannerState, scope: Scope) -> anyhow::Result<Target> {
    let pin = state
        .published_spec_pin
        .clone()
        .ok_or_else(|| anyhow::anyhow!("No published specification revision is available"))?;
    let reader = crate::core::published_specs::PublishedSpecReader::for_primary_repository(
        &state.repo_root,
    )?;
    reader.ensure_local_ref_unchanged(&pin)?;
    let docs = reader.read_documentation(&pin)?;
    let source_documents = match &scope {
        Scope::Product => {
            let product_docs = docs
                .iter()
                .filter(|doc| doc.path.starts_with("docs/koolade/product/"))
                .collect::<Vec<_>>();
            product_docs
                .iter()
                .map(|doc| SourceDocument {
                    path: doc.path.clone(),
                    sha256: doc.content_sha256.clone(),
                })
                .collect::<Vec<_>>()
        }
        Scope::Feature { display_id, uid } => {
            let Some(_) = state.active_features.iter().find(|(id, content)| {
                id == display_id
                    && crate::domain::ArtifactIdentity::from_markdown(content)
                        .ok()
                        .flatten()
                        .is_some_and(|identity| identity.uid == *uid)
            }) else {
                anyhow::bail!("The selected feature no longer has its recorded identity");
            };
            let path = format!("docs/koolade/features/{uid}.md");
            docs.iter()
                .find(|doc| doc.path == path)
                .map(|doc| {
                    vec![SourceDocument {
                        path: doc.path.clone(),
                        sha256: doc.content_sha256.clone(),
                    }]
                })
                .unwrap_or_default()
        }
    };
    let effective_document_sha256 = match &scope {
        Scope::Product => {
            let content = source_documents
                .iter()
                .map(|doc| format!("{}:{}\n", doc.path, doc.sha256))
                .collect::<String>();
            format!("{:x}", Sha256::digest(content.as_bytes()))
        }
        Scope::Feature { display_id, uid } => {
            let content = state
                .active_features
                .iter()
                .find(|(id, content)| {
                    id == display_id
                        && crate::domain::ArtifactIdentity::from_markdown(content)
                            .ok()
                            .flatten()
                            .is_some_and(|identity| identity.uid == *uid)
                })
                .map(|(_, content)| content.as_bytes())
                .expect("feature identity was validated above");
            format!("{:x}", Sha256::digest(content))
        }
    };
    Ok(Target {
        scope,
        published_pin: pin,
        source_documents,
        effective_document_sha256,
    })
}

pub fn validate_target(state: &PlannerState, expected: &Target) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.published_spec_pin.as_ref() == Some(&expected.published_pin),
        "The published specification changed. Refresh it before retrying this conversation."
    );
    let actual = capture_target(state, expected.scope.clone())?;
    anyhow::ensure!(
        actual == *expected,
        "The selected specification changed. Refresh it before retrying this conversation."
    );
    Ok(())
}

pub fn validate_proposal(
    target: &Target,
    normalized: &mut crate::core::validation::NormalizedTurn,
    state: &PlannerState,
) -> Result<(), String> {
    use crate::domain::ChangeStatus;
    let expected_prefix = match &target.scope {
        Scope::Product => "product:".to_owned(),
        Scope::Feature { display_id, uid } => {
            let matches = state.active_features.iter().any(|(id, content)| {
                id == display_id
                    && crate::domain::ArtifactIdentity::from_markdown(content)
                        .ok()
                        .flatten()
                        .is_some_and(|identity| identity.uid == *uid)
            });
            if !matches {
                return Err("The selected feature identity changed; no proposal was saved.".into());
            }
            format!("feature:{display_id}")
        }
    };
    if normalized.document_updates.is_empty()
        || normalized
            .document_updates
            .iter()
            .any(|(id, _)| match &target.scope {
                Scope::Product => !id.starts_with(&expected_prefix),
                Scope::Feature { .. } => id != &expected_prefix,
            })
    {
        return Err("The proposal did not stay within the selected specification scope; no changes were saved.".into());
    }
    if let Scope::Product = &target.scope {
        validate_local_product_questions(&normalized.added)?;
        if !normalized.change_status_updates.is_empty() {
            return Err("A product baseline cannot change feature lifecycle status.".into());
        }
    } else if !normalized.added.is_empty() {
        return Err(
            "A feature proposal cannot create unrelated product questions; no changes were saved."
                .into(),
        );
    }
    if !normalized.planning_tasks.is_empty()
        || !normalized.updates.is_empty()
        || !normalized.resolved.is_empty()
        || normalized.requested_action.is_some()
        || normalized.follow_up_task.is_some()
        || normalized.next_question_id.is_some()
        || normalized.workflow.is_some()
        || normalized.task_batch.is_some()
        || normalized.plan_comparison.is_some()
        || !normalized.additional_planning_artifacts.is_empty()
    {
        return Err("A Specification proposal can only create a local document draft; no changes were saved.".into());
    }
    if let Scope::Feature { display_id, .. } = &target.scope {
        let current = state
            .active_features
            .iter()
            .find(|(id, _)| id == display_id)
            .and_then(|(_, markdown)| {
                crate::domain::ChangeMetadata::require_markdown(markdown).ok()
            })
            .map(|metadata| metadata.status);
        if current.is_some_and(|status| !status.can_transition_to(ChangeStatus::Draft)) {
            return Err(
                "The selected feature cannot become a draft from its current lifecycle state."
                    .into(),
            );
        }
        normalized
            .change_status_updates
            .insert(display_id.clone(), ChangeStatus::Draft);
    }
    Ok(())
}

fn validate_local_product_questions(items: &[crate::domain::OpenItem]) -> Result<(), String> {
    use crate::domain::{Authority, ItemKind, ItemStatus};
    if items.len() > 5 {
        return Err(
            "A product baseline can add at most five local decision questions per turn.".into(),
        );
    }
    if items.iter().any(|item| {
        item.authority != Authority::Human
            || !matches!(
                item.kind,
                ItemKind::Question | ItemKind::Ambiguity | ItemKind::Assumption
            )
            || item.status != ItemStatus::Open
            || item.feature_id.is_some()
            || item.feature_uid.is_some()
            || item.task_intent_uid.is_some()
            || item.task_intent_cycle_uid.is_some()
            || !item.blocked_by.is_empty()
    }) {
        return Err("Product baseline cards must be open, local Human decisions without task or feature links.".into());
    }
    Ok(())
}
