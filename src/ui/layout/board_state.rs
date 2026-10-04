use crate::ui::Surface;

pub(crate) fn task_board_column(s: &dyn Surface, key: &str) -> usize {
    if !s.implementation_active(key) && s.implementation_failure(key).is_some() {
        return 3;
    }
    let base = crate::core::implementation::board_column(
        s.implementation_state(key),
        s.implementation_active(key),
    );
    // An active implementation owns its status even if an older chat failed.
    if s.implementation_active(key) {
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
