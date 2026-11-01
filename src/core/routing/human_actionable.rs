use crate::domain::{Authority, CurrentUser, OpenItem, Stakeholders};

/// Open items that may create a user-facing answer request. Review items stay
/// visible to the operator, but are routed to an internal reviewer rather than
/// presented as a decision the user owes.
pub fn human_actionable_items<'a>(
    items: &'a [OpenItem],
    user: &CurrentUser,
    stakeholders: &Stakeholders,
) -> Vec<&'a OpenItem> {
    super::eligible_items(items, user, stakeholders)
        .into_iter()
        .filter(|item| item.authority == Authority::Human || item.is_ownership_gap())
        .collect()
}
