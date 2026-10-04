//! Task conversation cards, discussion history, and their interaction model.

mod composer;
mod conversation;
mod decision;
mod status;
mod transcript;

#[cfg(test)]
mod tests;

pub fn paint(ui: &mut egui::Ui, s: &mut dyn crate::ui::Surface, key: &str, expanded: bool) -> bool {
    let board = s.planning_board();
    conversation::paint_with_board(ui, s, key, expanded, &board)
}

pub(crate) use conversation::paint_with_board;
pub(crate) use status::board_column;
#[cfg(test)]
pub(crate) use status::{Reply, split_reply};
pub use transcript::paint_history;
pub(crate) use transcript::paint_history_messages;
#[cfg(test)]
pub(crate) use transcript::transcript;
