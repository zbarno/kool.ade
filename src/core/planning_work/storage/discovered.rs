use super::super::{Work, validate};

pub fn append_discovered(
    drafts: &[crate::harness::PlanningTaskDraft],
) -> anyhow::Result<Vec<Work>> {
    let mut items = Vec::with_capacity(drafts.len());
    for draft in drafts {
        let mut work = Work::new(
            String::new(),
            draft.title.trim().to_owned(),
            draft.description.trim().to_owned(),
            "Discovered during repository documentation review; triage before acting.".into(),
        );
        work.key = format!("task:{}", work.uid);
        work.kind = draft.kind;
        work.status = draft.status;
        items.push(work);
    }
    validate(&items)?;
    Ok(items)
}

pub fn find(state: &crate::core::state::PlannerState, key: &str) -> Option<Work> {
    super::load(&state.planning_store)
        .ok()?
        .into_iter()
        .find(|item| item.key == key)
}
