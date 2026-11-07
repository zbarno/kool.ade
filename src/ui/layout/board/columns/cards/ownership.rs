use crate::{
    core::planning_work::{Work, WorkStatus},
    domain::{Authority, OpenItem},
};

pub(super) fn agent_pill(ui: &mut egui::Ui) {
    crate::ui::theme::badge(
        ui,
        "AGENT",
        crate::ui::theme::ACCENT_SOFT,
        crate::ui::theme::TEXT,
    );
}

pub(super) fn work_is_agent(work: &Work) -> bool {
    work.operational_recovery.is_some()
        || matches!(work.status, WorkStatus::Todo | WorkStatus::InProgress)
        || work.kind.starts_task_intent() && work.status != WorkStatus::InReview
}

pub(super) fn item_is_agent(item: &OpenItem, saved: bool) -> bool {
    saved || item.authority == Authority::Agent
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responsibility_follows_authority_and_saved_answer_handoff() {
        let mut item = OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "Product".into(),
            "Choose behavior".into(),
            String::new(),
        );
        item.authority = Authority::Human;
        assert!(!item_is_agent(&item, false));
        assert!(item_is_agent(&item, true));
        item.authority = Authority::Agent;
        assert!(item_is_agent(&item, false));
        item.authority = Authority::Review;
        assert!(!item_is_agent(&item, false));
    }
}
