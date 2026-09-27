use crate::domain::{Authority, OpenItem};

/// A model may resolve a Human-owned item only when the application scoped
/// the turn to that item as the user's active task reply.
pub(super) fn unauthorized_resolution(
    item: &OpenItem,
    user_replied_item_ids: &[String],
) -> Option<String> {
    (item.authority == Authority::Human && !user_replied_item_ids.iter().any(|id| id == &item.id))
        .then(|| {
            format!(
                "{}: Human authority requires an explicit user reply in that item's conversation",
                item.id
            )
        })
}
