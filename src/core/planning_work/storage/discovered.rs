use super::super::Work;

pub fn append_discovered(
    drafts: &[crate::harness::PlanningTaskDraft],
    intent_context: Option<&crate::core::validation::TaskIntentWorkContext>,
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
        if let Some(context) = intent_context {
            work.parent_uid = Some(context.parent_work_uid.clone());
            work.task_intent_uid = Some(context.root_uid.clone());
            work.task_intent_cycle_uid = context.cycle_uid.clone();
        }
        items.push(work);
    }
    Ok(items)
}

pub fn find(state: &crate::core::state::PlannerState, key: &str) -> Option<Work> {
    super::load(&state.planning_store)
        .ok()?
        .into_iter()
        .find(|item| item.key == key)
}
