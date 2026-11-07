//! Structured, deterministic pages over the complete local Manager board.
use sha2::Digest;

pub(crate) mod entries;
#[cfg(test)]
mod tests;

pub(crate) use entries::TaskIndexEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskIndexPage {
    pub index_revision: String,
    pub planning_revision: String,
    pub total_entries: usize,
    pub offset: usize,
    pub entries: Vec<TaskIndexEntry>,
    pub next_offset: Option<usize>,
    pub omitted_details: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskIndexError {
    StaleRevision { expected: String, actual: String },
}

/// Query a revision-fenced deterministic page. A caller must pass the prior
/// page's index_revision when continuing pagination.
pub(crate) fn query(
    project: &crate::app::session::Project,
    expected_revision: Option<&str>,
    offset: usize,
    limit: usize,
    focus_ids: &[String],
) -> Result<TaskIndexPage, TaskIndexError> {
    let entries = entries::build(project, focus_ids);
    let index_revision = revision(&entries);
    if let Some(expected) = expected_revision
        && expected != index_revision
    {
        return Err(TaskIndexError::StaleRevision {
            expected: expected.into(),
            actual: index_revision,
        });
    }
    let total_entries = entries.len();
    let selected = entries
        .into_iter()
        .skip(offset)
        .take(limit.clamp(1, 256))
        .collect::<Vec<_>>();
    let next = offset.saturating_add(selected.len());
    Ok(TaskIndexPage {
        index_revision,
        planning_revision: project.state.baseline_planning_revision.clone(),
        total_entries,
        offset,
        entries: selected,
        next_offset: (next < total_entries).then_some(next),
        omitted_details: total_entries.saturating_sub(next),
    })
}

fn revision(entries: &[TaskIndexEntry]) -> String {
    let encoded = serde_json::to_vec(entries).expect("Task index entries serialize");
    format!("{:x}", sha2::Sha256::digest(encoded))
}

pub(super) fn render(
    project: &crate::app::session::Project,
    events: &[String],
) -> (String, String) {
    const PROMPT_INDEX_BYTES: usize = 5 * 1024;
    let page = super::super::task_index_page(project, None, 0, 256, events)
        .expect("unfenced first index page");
    let mut included = Vec::new();
    let mut used = 0usize;
    for entry in &page.entries {
        let text = entry.prompt_line();
        let bytes = text.len().saturating_add(usize::from(!included.is_empty()));
        if used.saturating_add(bytes) > PROMPT_INDEX_BYTES {
            break;
        }
        used += bytes;
        included.push(text);
    }
    let included_count = included.len();
    let rows = included.join("\n");
    let omitted = page.total_entries.saturating_sub(included_count);
    let next_prompt_offset = (included_count < page.total_entries).then_some(included_count);
    let omitted_ids = page.entries[included_count..]
        .iter()
        .take(20)
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    let metrics = format!(
        "Board index: {} entries; index revision {}; planning revision {}; active planning turn {:?}; included {} entries ({} bytes, budget {PROMPT_INDEX_BYTES}); omitted {omitted} by prompt byte budget, first omitted IDs {:?}; query next page at offset {:?} using this index revision.",
        page.total_entries,
        page.index_revision,
        page.planning_revision,
        project.active_planning_work,
        included_count,
        used,
        omitted_ids,
        next_prompt_offset
    );
    (metrics, rows)
}
