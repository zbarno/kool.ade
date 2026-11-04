//! Routes planning questions to the person running this Kool.ad/e installation.
//!
//! Human questions belong to the local user; Agent and Review items remain
//! visible without interrupting that user. Open prerequisites continue to
//! fence dependent questions.

use crate::domain::{Authority, CurrentUser, OpenItem, Priority};

mod human_actionable;
pub use human_actionable::human_actionable_items;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    LocalUser,
    NotEligible,
}

impl Eligibility {
    pub const fn is_eligible(self) -> bool {
        matches!(self, Self::LocalUser)
    }
}

pub fn evaluate(item: &OpenItem, _user: &CurrentUser) -> Eligibility {
    if item.authority == Authority::Agent {
        Eligibility::NotEligible
    } else {
        Eligibility::LocalUser
    }
}

pub fn eligible_items<'a>(items: &'a [OpenItem], user: &CurrentUser) -> Vec<&'a OpenItem> {
    items
        .iter()
        .filter(|item| {
            item.status == crate::domain::ItemStatus::Open
                && evaluate(item, user).is_eligible()
                && !has_open_prerequisite(items, item)
        })
        .collect()
}

pub fn has_open_prerequisite(items: &[OpenItem], item: &OpenItem) -> bool {
    item.blocked_by.iter().any(|dependency| {
        items.iter().any(|candidate| {
            candidate.id == *dependency && candidate.status == crate::domain::ItemStatus::Open
        })
    })
}

pub fn recommended_next<'a>(items: &'a [OpenItem], user: &CurrentUser) -> Option<&'a OpenItem> {
    eligible_items(items, user)
        .into_iter()
        .filter(|item| item.authority == Authority::Human && item.priority == Priority::Blocking)
        .min_by_key(|item| (item.priority.rank(), item.id.clone()))
}

#[cfg(test)]
mod tests;
