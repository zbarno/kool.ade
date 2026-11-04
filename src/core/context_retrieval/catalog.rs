use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::core::context_build::clip;
use crate::core::published_specs::{PublishedSpecPin, PublishedSpecReader};
use crate::core::repo_overview;
use crate::core::state::PlannerState;
use crate::domain::OpenItem;
use crate::harness::RetrievalPlan;

use super::areas;
use super::{ContextSelection, RetrievedDocument};

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
    repo_root: PathBuf,
    published_documents: BTreeMap<String, super::published_navigation::PublishedCandidate>,
    published_reader: Option<PublishedSpecReader>,
    published_pin: Option<PublishedSpecPin>,
    items: BTreeMap<String, OpenItem>,
    areas: BTreeMap<String, String>,
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
        let overview = repo_overview::scan(&state.repo_root);
        let areas = areas::catalog(&state.repo_root, &overview);
        Self {
            repo_root: state.repo_root.clone(),
            published_documents,
            published_reader: Some(reader),
            published_pin: Some(pin),
            items,
            areas,
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
        out.push_str("\nOPEN ITEMS (exact item ID | question | reason)\n");
        for (id, item) in &self.items {
            out.push_str(&format!(
                "- {id} | {} | {}\n",
                clip(&item.question, 180),
                clip(&item.reason, 180)
            ));
        }
        out.push_str("\nREPOSITORY AREAS (exact repo-relative directory | files)\n");
        for (path, files) in &self.areas {
            out.push_str(&format!("- {path} | {files}\n"));
        }
        clip(&out, MAX_CATALOG_CHARS)
    }

    pub(super) fn resolve(&self, plan: RetrievalPlan) -> ContextSelection {
        let mut selection = ContextSelection::default();
        let mut used_budget = 0;
        let mut seen = BTreeSet::new();
        for id in plan.documents.into_iter().take(MAX_DOCUMENT_SELECTIONS * 2) {
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
            if let Some(content) = areas::load(&path, &self.areas, &self.repo_root) {
                selection
                    .repository_areas
                    .push(super::RetrievedArea { path, content });
            }
        }
        selection
    }
}
