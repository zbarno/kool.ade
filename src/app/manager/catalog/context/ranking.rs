use crate::harness::prompt_catalog::EvidenceItem;

mod relevance;
use relevance::{category, rank};
pub(crate) use relevance::{overlap, terms};

const MAX_OMITTED_ID_BYTES: usize = 400;
const MAX_SELECTED_EVIDENCE_BYTES: usize = 48 * 1024;
const MAX_SELECTED_SOURCES: usize = 12;

fn evidence_cost(item: &EvidenceItem) -> usize {
    #[derive(serde::Serialize)]
    struct Quoted<'a> {
        id: &'a str,
        origin: crate::harness::prompt_catalog::EvidenceOrigin,
        source: &'a str,
        content: &'a str,
    }
    serde_json::to_vec(&Quoted {
        id: &item.id,
        origin: item.origin,
        source: &item.source,
        content: &item.content,
    })
    .map(|quoted| quoted.len() + 1)
    .unwrap_or(usize::MAX)
}

/// Builds an application-owned ordering hint. It is advisory only; evidence
/// IDs bind every listed source to the already validated manifest.
pub(crate) fn resolve_sources(
    evidence: Vec<EvidenceItem>,
    goal: &str,
) -> (Vec<EvidenceItem>, Vec<String>) {
    let category = category(goal);
    let goal_terms = terms(goal);
    let mut sources = evidence
        .iter()
        .map(|item| {
            let rank = rank(&item.id, category);
            let relevance = overlap(&goal_terms, &format!("{} {}", item.source, item.content));
            (rank, std::cmp::Reverse(relevance), item.id.clone())
        })
        .collect::<Vec<_>>();
    sources.sort();
    let total_sources = sources.len();
    let mut by_id = evidence
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect::<std::collections::HashMap<_, _>>();
    let mut selected_ids = Vec::new();
    let mut selected_bytes = 0usize;
    for (rank, _, id) in sources {
        if selected_ids.len() >= MAX_SELECTED_SOURCES {
            continue;
        }
        let Some(item) = by_id.get(&id) else {
            continue;
        };
        let cost = evidence_cost(item);
        if selected_bytes.saturating_add(cost) > MAX_SELECTED_EVIDENCE_BYTES {
            continue;
        }
        selected_bytes += cost;
        selected_ids.push((rank, id));
    }
    // Keep the workspace summary when it fits, even if lower-ranked sources
    // consumed the remaining budget first.
    if !selected_ids.iter().any(|(_, id)| id == "workspace-summary")
        && let Some(summary) = by_id.get("workspace-summary")
    {
        let cost = evidence_cost(summary);
        if (selected_ids.len() >= MAX_SELECTED_SOURCES
            || selected_bytes.saturating_add(cost) > MAX_SELECTED_EVIDENCE_BYTES)
            && let Some((index, (_, removed_id))) = selected_ids
                .iter()
                .enumerate()
                .rev()
                .find(|(_, (_, id))| id != "workspace-summary")
        {
            selected_bytes = selected_bytes
                .saturating_sub(by_id.get(removed_id).map(evidence_cost).unwrap_or_default());
            selected_ids.remove(index);
        }
        if selected_bytes.saturating_add(cost) <= MAX_SELECTED_EVIDENCE_BYTES
            && selected_ids.len() < MAX_SELECTED_SOURCES
        {
            selected_ids.push((rank(&summary.id, category), summary.id.clone()));
        }
    }
    selected_ids.sort();
    let selected_count = selected_ids.len();
    let resolved = selected_ids
        .iter()
        .filter_map(|(_, id)| by_id.remove(id))
        .collect::<Vec<_>>();
    let mut result = selected_ids
        .into_iter()
        .map(|(rank, id)| format!("rank={rank} evidence_id={id}"))
        .collect::<Vec<_>>();
    if total_sources > selected_count {
        let mut omitted = by_id.keys().cloned().collect::<Vec<_>>();
        omitted.sort();
        let omitted_ids = omitted.join(",");
        let clipped = crate::core::context_build::clip_bytes(&omitted_ids, MAX_OMITTED_ID_BYTES);
        result.push(format!(
            "omitted_sources={} omitted_ids={} omitted_ids_truncated={}",
            total_sources - selected_count,
            clipped,
            clipped.len() < omitted_ids.len()
        ));
    }
    (resolved, result)
}

pub(crate) fn telemetry(
    evidence: &[EvidenceItem],
    selection: &[String],
    elapsed: std::time::Duration,
) -> crate::harness::ManagerContextTelemetryV1 {
    let selected_source_ids = selection
        .iter()
        .filter_map(|entry| {
            entry
                .split_once("evidence_id=")
                .map(|(_, id)| id.to_owned())
        })
        .collect::<Vec<_>>();
    let omitted_entry = selection
        .iter()
        .find(|entry| entry.starts_with("omitted_sources="));
    let omitted_source_count = omitted_entry
        .and_then(|entry| entry.strip_prefix("omitted_sources="))
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(|count| count.parse().ok())
        .unwrap_or_default();
    let omitted_ids_truncated =
        omitted_entry.is_some_and(|entry| entry.ends_with("omitted_ids_truncated=true"));
    let omitted_source_ids = omitted_entry
        .and_then(|entry| entry.split_once("omitted_ids=").map(|(_, tail)| tail))
        .and_then(|tail| {
            tail.split_once(" omitted_ids_truncated=")
                .map(|(ids, _)| ids)
        })
        .unwrap_or_default()
        .to_owned();
    let selected_evidence_bytes = evidence
        .iter()
        .map(|item| item.content.len() as u64)
        .sum::<u64>();
    let profile_cache_hits = evidence
        .iter()
        .filter(|item| item.id == "repository-architecture-profile")
        .filter_map(|item| {
            item.source
                .split_once("cache_hits=")
                .map(|(_, value)| value)
        })
        .filter_map(|value| value.parse::<u32>().ok())
        .sum();
    crate::harness::ManagerContextTelemetryV1 {
        selected_source_ids,
        omitted_source_count,
        omitted_source_ids,
        omitted_ids_truncated,
        selected_evidence_bytes,
        estimated_context_tokens: selected_evidence_bytes.saturating_add(3) / 4,
        preparation_millis: elapsed.as_millis().min(u64::MAX as u128) as u64,
        profile_cache_hits,
        uncertainty_reported: None,
        answerable_without_user_question: None,
    }
}
