//! Durable board accounting for planning requests, including requests that
//! have not yet produced a feature specification.
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE: &str = crate::artifacts::layout::canonical::WORK;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Work {
    pub key: String,
    pub title: String,
    pub request: String,
    pub column: usize,
    pub feature: Option<String>,
    pub detail: String,
}

pub fn load(repo: &Path) -> anyhow::Result<Vec<Work>> {
    match std::fs::read(repo.join(FILE)) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

pub fn save(repo: &Path, work: &[Work]) -> anyhow::Result<()> {
    crate::artifacts::task_docs::safe_directory(repo, crate::artifacts::layout::canonical::STATE)?;
    let path = repo.join(FILE);
    anyhow::ensure!(
        !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()),
        "Linked planning work ledger"
    );
    crate::artifacts::atomic_write(&path, &serde_json::to_string_pretty(work)?)
}

/// Feature documents are durable board identity even after the chat is gone.
pub fn cards(state: &crate::core::state::PlannerState, work: &[Work]) -> Vec<Work> {
    let mut cards = work.to_vec();
    for card in &mut cards {
        if card
            .feature
            .as_ref()
            .is_some_and(|id| !state.active_features.iter().any(|(f, _)| f == id))
            && card.column != 3
        {
            card.column = 4;
        }
    }
    for (id, body) in &state.active_features {
        let status = crate::domain::ChangeMetadata::require_markdown(body)
            .map(|metadata| metadata.status)
            .expect("loaded active changes always have validated structured status");
        let column = if matches!(
            status,
            crate::domain::ChangeStatus::Ready
                | crate::domain::ChangeStatus::Implementing
                | crate::domain::ChangeStatus::Reconciliation
        ) {
            4
        } else {
            1
        };
        let title = body
            .lines()
            .next()
            .unwrap_or(id)
            .trim_start_matches('#')
            .trim();
        if let Some(card) = cards.iter_mut().find(|w| w.feature.as_ref() == Some(id)) {
            if card.column != 3 {
                card.column = column;
            }
            card.title = format!("Plan {title}");
        } else {
            cards.push(Work {
                key: format!("feature:{id}"),
                title: format!("Plan {title}"),
                request: body.clone(),
                column,
                feature: Some(id.clone()),
                detail: "Feature planning".into(),
            });
        }
    }
    cards
}

pub fn context(state: &crate::core::state::PlannerState, key: &str) -> Option<String> {
    let work = load(&state.repo_root).ok()?;
    let card = cards(state, &work).into_iter().find(|w| w.key == key)?;
    let feature = card
        .feature
        .as_ref()
        .and_then(|id| state.active_features.iter().find(|(f, _)| f == id));
    Some(format!(
        "{}\n{}\n{}\n{}",
        card.title,
        card.request,
        card.detail,
        feature
            .map(|(_, body)| body.as_str())
            .unwrap_or("No feature specification recorded yet.")
    ))
}
