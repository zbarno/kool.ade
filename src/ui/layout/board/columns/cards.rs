use super::super::super::*;

mod clarification;
mod feature;
mod item;
mod machine_group;
mod ownership;
mod pr_task;
mod setup;
mod task;
mod task_semantics;
mod work;

pub(super) use clarification::clarification;
pub(super) use feature::approval;
pub(super) use feature::plan_review;
pub(super) use item::item;
pub(super) use machine_group::machine_group;
pub(super) use pr_task::{pr_task, pr_task_column};
pub(super) use setup::setup;
pub(super) use task::task;
pub(super) use work::work;

pub(super) fn task_card_column(
    surface: &dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    document: &crate::artifacts::task_docs::TaskDocument,
) -> usize {
    if board
        .cancelled
        .contains(&crate::persistence::cancelled_work::task_id(document))
    {
        return 4;
    }
    let fallback = task_document_board_column(surface, document);
    task_semantics::task_board_column(
        board.lifecycle_tasks.get(&document.path).copied(),
        board.lifecycle_pr_statuses.get(&document.path).copied(),
        fallback,
    )
}
