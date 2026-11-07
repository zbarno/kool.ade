use crate::{core::planning_work::Work, ui::planning_board::ViewModel};

/// Owned work stays on the board until its whole intent is explicitly archived.
/// A missing owner is also visible; it must never strand work in a details-only map.
pub(super) fn visible(board: &ViewModel, key: &str, owner: Option<&str>) -> bool {
    board.is_visible(key, owner)
}

pub(super) fn work_column(board: &ViewModel, work: &Work) -> usize {
    if board
        .cancelled
        .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
    {
        4
    } else {
        work.board_column()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_individually_archived_child_remains_on_its_intent_board() {
        let mut board = ViewModel::default();
        board.archived.insert("child".into());
        assert!(!visible(&board, "child", None));
        assert!(visible(&board, "child", Some("missing-or-live-owner")));
        let mut root = Work::new(
            "root".into(),
            "Request".into(),
            "Goal".into(),
            String::new(),
        );
        root.task_intent_uid = Some(root.uid.clone());
        root.status = crate::core::planning_work::WorkStatus::Done;
        root.kind = crate::core::planning_work::WorkKind::Question;
        root.task_intent_archived_at_ms = Some(1);
        let uid = root.uid.clone();
        board.planning_work.push(root);
        assert!(!visible(&board, "child", Some(&uid)));
    }
}
