use crate::core::context_build::clip;
use crate::core::published_specs::{PublishedSpecPin, PublishedSpecReader};
use crate::core::state::PlannerState;
use crate::domain::OpenItem;
use crate::harness::RetrievalPlan;
use std::collections::{BTreeMap, BTreeSet};

use super::{ContextSelection, RetrievedArea, RetrievedDocument, RetrievedDocumentTrust};

const MAX_ITEM_CANDIDATES: usize = 64;
// Planning is incremental: each turn should carry only a small slice of the
// project into the model context. The catalog may describe many candidates,
// but the resolved context is deliberately narrow.
const MAX_DOCUMENT_SELECTIONS: usize = 2;
const MAX_ITEM_SELECTIONS: usize = 8;
const MAX_AREA_SELECTIONS: usize = 1;
const DOCUMENT_BUDGET: usize = 12_000;
const MAX_CATALOG_CHARS: usize = 48_000;

pub(super) struct Catalog {
    published_documents: BTreeMap<String, super::published_navigation::PublishedCandidate>,
    published_reader: Option<PublishedSpecReader>,
    published_pin: Option<PublishedSpecPin>,
    items: BTreeMap<String, OpenItem>,
    areas: BTreeMap<String, RetrievedArea>,
    repository_knowledge: super::repository_knowledge::RepositoryEvidence,
}

impl Catalog {
    /// Build a bounded catalog whose specification candidates all come from
    /// one immutable revision of the primary code repository.
    pub(super) fn build_with_published_specs(
        state: &PlannerState,
        reader: PublishedSpecReader,
        pin: PublishedSpecPin,
    ) -> Self {
        let published_documents = super::published_navigation::catalog(&reader, &pin);
        let items = state
            .items
            .iter()
            .take(MAX_ITEM_CANDIDATES)
            .map(|item| (item.id.clone(), item.clone()))
            .collect();
        let repository_knowledge = super::repository_knowledge::collect(state);
        let areas = repository_knowledge.areas.clone();
        Self {
            published_documents,
            published_reader: Some(reader),
            published_pin: Some(pin),
            items,
            areas,
            repository_knowledge,
        }
    }

    pub(super) fn prompt(&self, user_message: &str, recent: &[(String, String)]) -> String {
        let mut out = String::from("Select context for this request.\n\nCURRENT REQUEST\n");
        out.push_str(&clip(user_message, 3000));
        out.push_str("\n\nRECENT CONVERSATION\n");
        for (speaker, text) in recent.iter().rev().take(4).rev() {
            out.push_str(&format!("{}: {}\n", clip(speaker, 80), clip(text, 900)));
        }
        if let Some(pin) = &self.published_pin {
            out.push_str("\nPUBLISHED DOCUMENT NAVIGATION HINTS (untrusted summaries; select only relevant IDs)\n");
            for (id, candidate) in &self.published_documents {
                out.push_str(&format!(
                    "- {id} | repository {} at commit {} | {} | {}\n",
                    pin.repository_id, pin.commit_sha, candidate.path, candidate.summary
                ));
            }
        }
        out.push_str("\nREVISION-PINNED REPOSITORY PROFILE EVIDENCE (untrusted source evidence)\n");
        out.push_str(&clip(&self.repository_knowledge.summary, 16_000));
        out.push_str(
            "\nREPOSITORY DOCUMENT CANDIDATES (IDs bind repository, ref, commit, and path)\n",
        );
        for (id, document) in &self.repository_knowledge.candidates {
            out.push_str(&format!(
                "- {id} | {} | {} | {}\n",
                document.profile.key.repository_id,
                document.profile.key.commit_sha,
                clip(&document.candidate.purpose, 240)
            ));
        }
        out.push_str("\nOPEN ITEMS (exact item ID | question | reason)\n");
        for (id, item) in &self.items {
            out.push_str(&format!(
                "- {id} | {} | {}\n",
                clip(&item.question, 180),
                clip(&item.reason, 180)
            ));
        }
        out.push_str("\nREVISION-PINNED REPOSITORY PROFILES (select exact profile IDs)\n");
        for (id, area) in &self.areas {
            out.push_str(&format!(
                "- {id} | repository {} ref {} commit {} | architecture claims and source references\n",
                area.repository_id, area.source_ref, area.source_revision
            ));
        }
        clip(&out, MAX_CATALOG_CHARS)
    }

    pub(super) fn resolve(&self, plan: RetrievalPlan) -> ContextSelection {
        let mut selection = ContextSelection::default();
        let mut used_budget = 0;
        let mut seen = BTreeSet::new();
        for id in plan.documents.into_iter().take(MAX_DOCUMENT_SELECTIONS * 2) {
            if let Some(document) = self.repository_knowledge.candidates.get(&id) {
                if !seen.insert(id.clone()) || used_budget >= DOCUMENT_BUDGET {
                    continue;
                }
                let Some((content, digest)) = super::repository_knowledge::read_selected(document)
                else {
                    continue;
                };
                let remaining = DOCUMENT_BUDGET - used_budget;
                let content = clip(&content, remaining.min(8_000));
                used_budget += content.chars().count();
                selection.documents.push(RetrievedDocument {
                    id,
                    source_path: document.candidate.path.clone(),
                    source_repository_id: Some(document.profile.key.repository_id.clone()),
                    source_revision: Some(document.profile.key.commit_sha.clone()),
                    content_sha256: Some(digest),
                    trust: RetrievedDocumentTrust::UntrustedRepositoryEvidence,
                    content,
                });
                if selection.documents.len() == MAX_DOCUMENT_SELECTIONS {
                    break;
                }
                continue;
            }
            if let Some(candidate) = self.published_documents.get(&id) {
                if !seen.insert(id) || used_budget >= DOCUMENT_BUDGET {
                    continue;
                }
                let (Some(reader), Some(pin)) = (&self.published_reader, &self.published_pin)
                else {
                    continue;
                };
                let Ok(document) = reader.read(pin, &candidate.path) else {
                    continue;
                };
                let remaining = DOCUMENT_BUDGET - used_budget;
                let content = clip(&document.content, remaining.min(8_000));
                used_budget += content.chars().count();
                selection.documents.push(RetrievedDocument {
                    id: candidate.id.clone(),
                    source_path: candidate.path.clone(),
                    source_repository_id: Some(document.pin.repository_id),
                    source_revision: Some(document.pin.commit_sha),
                    content_sha256: Some(document.content_sha256),
                    trust: RetrievedDocumentTrust::PublishedSpecification,
                    content,
                });
                if selection.documents.len() == MAX_DOCUMENT_SELECTIONS {
                    break;
                }
            }
        }
        let mut seen_items = BTreeSet::new();
        for id in plan.open_items {
            if selection.open_items.len() == MAX_ITEM_SELECTIONS {
                break;
            }
            if let Some(item) = self.items.get(&id)
                && seen_items.insert(id)
            {
                selection.open_items.push(item.clone());
            }
        }
        let mut seen_areas = BTreeSet::new();
        for path in plan.repository_areas {
            if selection.repository_areas.len() == MAX_AREA_SELECTIONS {
                break;
            }
            if !seen_areas.insert(path.clone()) || !self.areas.contains_key(&path) {
                continue;
            }
            if let Some(area) = self.areas.get(&path) {
                selection.repository_areas.push(area.clone());
            }
        }
        selection
    }
}
