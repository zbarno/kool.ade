use crate::ui::Surface;

pub(crate) fn task_document_board_column(
    s: &dyn Surface,
    document: &crate::artifacts::task_docs::TaskDocument,
) -> usize {
    let key = document.path.as_str();
    let active = s.implementation_active(key);
    let local = s.implementation_state(key);
    let base = if active {
        crate::core::implementation::board_column(local, active)
    } else if let Some(task_state) = &document.task_state {
        task_state.status.board_column()
    } else if s.implementation_failure(key).is_some() {
        3
    } else {
        crate::core::implementation::board_column(local, active)
    };
    if active {
        base
    } else {
        crate::ui::task_chat::board_column(base, s.task_messages(key), s.task_chat_active(key))
    }
}

pub(crate) fn planning_column(
    item: &crate::domain::item::OpenItem,
    items: &[crate::domain::item::OpenItem],
) -> usize {
    if item.status == crate::domain::item::ItemStatus::Resolved {
        4
    } else if crate::core::routing::has_open_prerequisite(items, item) {
        0
    } else if item.is_ownership_gap()
        || matches!(
            item.authority,
            crate::domain::Authority::Human | crate::domain::Authority::Review
        )
    {
        3
    } else {
        match item.authority {
            crate::domain::Authority::Agent => 0,
            crate::domain::Authority::Review => 2,
            crate::domain::Authority::Human => 0,
        }
    }
}

pub(crate) fn planning_parent_label(
    work: &crate::core::planning_work::Work,
    items: &[&crate::core::planning_work::Work],
) -> Option<String> {
    let parent_uid = work.parent_uid.as_ref()?;
    let parent = items.iter().find(|candidate| &candidate.uid == parent_uid);
    Some(format!(
        "Blocked by {}",
        parent
            .map(|item| item.title.as_str())
            .unwrap_or("parent work")
    ))
}
