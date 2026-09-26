use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::core::context_build::clip;
use crate::core::repo_overview;
use crate::core::state::PlannerState;
use crate::domain::OpenItem;
use crate::harness::RetrievalPlan;

use super::areas;
use super::documents;
use super::{CandidateDocument, ContextSelection, RetrievedDocument};

const MAX_CHANGE_CANDIDATES: usize = 64;
const MAX_ITEM_CANDIDATES: usize = 64;
const MAX_DOCUMENT_SELECTIONS: usize = 5;
const MAX_ITEM_SELECTIONS: usize = 8;
const MAX_AREA_SELECTIONS: usize = 3;
const DOCUMENT_BUDGET: usize = 24_000;
const MAX_CATALOG_CHARS: usize = 48_000;

pub(super) struct Catalog {
    repo_root: PathBuf,
    documents: BTreeMap<String, CandidateDocument>,
    items: BTreeMap<String, OpenItem>,
    areas: BTreeMap<String, String>,
}

impl Catalog {
    pub(super) fn build(state: &PlannerState) -> Self {
        let mut documents = documents::product_documents(&state.repo_root);
        for (id, body) in state.active_features.iter().take(MAX_CHANGE_CANDIDATES) {
            let Some(path) = documents::find_change_path(&state.repo_root, id, body) else {
                continue;
            };
            documents::insert_document(
                &mut documents,
                &state.repo_root,
                format!("change:{id}"),
                &path,
                body,
            );
        }
        documents::add_recent_changes(&state.repo_root, &mut documents);
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
            documents,
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
        out.push_str("\nAVAILABLE DOCUMENTS (logical ID | authoritative source | excerpt)\n");
        for (id, candidate) in &self.documents {
            out.push_str(&format!(
                "- {id} | {} | {}\n",
                candidate.source_path, candidate.excerpt
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
            let Some(candidate) = self.documents.get(&id) else {
                continue;
            };
            if !seen.insert(id) || used_budget >= DOCUMENT_BUDGET {
                continue;
            }
            let remaining = DOCUMENT_BUDGET - used_budget;
            let Some(document) =
                documents::load_selected(&self.repo_root, candidate, remaining.min(8_000))
            else {
                continue;
            };
            let content = clip(&document.content, remaining.min(8_000));
            used_budget += content.chars().count();
            selection.documents.push(RetrievedDocument {
                content,
                ..document
            });
            if selection.documents.len() == MAX_DOCUMENT_SELECTIONS {
                break;
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
