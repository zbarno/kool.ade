//! Read model for the planning and task board, assembled by the application
//! before rendering so the board does not query unrelated project state.

use std::collections::BTreeSet;

#[derive(Clone, Default)]
pub struct ViewModel {
    pub task_documents: Vec<crate::artifacts::task_docs::TaskDocument>,
    pub planning_work: Vec<crate::core::planning_work::Work>,
    pub planning_items: Vec<crate::domain::item::OpenItem>,
    pub eligible_item_ids: BTreeSet<String>,
    pub archived: BTreeSet<String>,
}

impl ViewModel {
    pub fn is_archived(&self, key: &str) -> bool {
        self.archived.contains(key)
    }
}
