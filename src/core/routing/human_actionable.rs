use crate::domain::{Authority, CurrentUser, OpenItem};

/// Open human items are questions for the local person using this installation.
pub fn human_actionable_items<'a>(items: &'a [OpenItem], user: &CurrentUser) -> Vec<&'a OpenItem> {
    super::eligible_items(items, user)
        .into_iter()
        .filter(|item| item.authority == Authority::Human)
        .collect()
}
