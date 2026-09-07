//! User-specific question routing (SPECIFICATION.md §11).
//!
//! An open item is *eligible* for the current user when exactly one holds:
//!
//! * its `assigned_to` matches the user's name or one of the user's groups,
//!   or
//! * its category is `General` (everyone's queue).
//!
//! Everything else remains visible in the panel but must NEVER be asked of
//! this user in chat. The app ENFORCES this on the agent's choice — an agent
//! suggestion violating the rules is dropped with a warning, not obeyed.

use crate::domain::{AssignmentMatch, CurrentUser, OpenItem, is_general_category};
/// Explanation of why an item may be posed to the current user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    ByName,
    ByGroup,
    General,
    /// Not this user's item (visible in panel only).
    NotEligible,
}

impl Eligibility {
    pub const fn is_eligible(self) -> bool {
        !matches!(self, Self::NotEligible)
    }
}

/// Evaluate one item against the user.
pub fn evaluate(item: &OpenItem, user: &CurrentUser) -> Eligibility {
    if is_general_category(&item.category) {
        return Eligibility::General;
    }
    if let Some(assignee) = &item.assigned_to {
        return match user.matches_assignment(assignee) {
            AssignmentMatch::ByName => Eligibility::ByName,
            AssignmentMatch::ByGroup => Eligibility::ByGroup,
            AssignmentMatch::Nobody => Eligibility::NotEligible,
        };
    }
    // No assignee recorded. If the item's category maps to the user somewhere
    // (their name or a group of theirs among the category's owners), route it.
    Eligibility::NotEligible
}

/// Filter a queue down to what the user may be interviewed with.
pub fn eligible_items<'a>(items: &'a [OpenItem], user: &CurrentUser) -> Vec<&'a OpenItem> {
    items
        .iter()
        .filter(|i| evaluate(i, user).is_eligible())
        .collect()
}

/// The single question the agent SHOULD pose next: highest priority, then
/// earliest ID. Ownership-gap items never count as interview questions.
pub fn recommended_next<'a>(items: &'a [OpenItem], user: &CurrentUser) -> Option<&'a OpenItem> {
    eligible_items(items, user)
        .into_iter()
        .filter(|i| !i.is_ownership_gap())
        .min_by_key(|i| (i.priority.rank(), i.id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ItemKind, Priority};

    fn item(id: &str, prio: Priority, category: &str, assignee: Option<&str>) -> OpenItem {
        OpenItem::new(
            id.into(),
            prio,
            ItemKind::Question,
            category.into(),
            assignee.map(String::from),
            format!("q-{id}"),
            "r".into(),
        )
    }

    fn queue() -> Vec<OpenItem> {
        vec![
            item("CLR-001", Priority::Blocking, "Security", Some("Morgan")),
            item("CLR-002", Priority::Blocking, "Product", Some("Product")),
            item("CLR-003", Priority::High, "Development", Some("Zach")),
            item("CLR-004", Priority::High, "General", None),
            item("CLR-005", Priority::Normal, "Development", Some("Alex")),
        ]
    }

    fn zach() -> CurrentUser {
        CurrentUser::new("Zach", vec!["Development".into(), "Architecture".into()])
    }

    #[test]
    fn spec_eleven_scenario_routes_correctly() {
        // Mirrors §11: Zach (Development) must get CLR-003 first, then General.
        let zs = zach();
        let q = queue();
        assert_eq!(recommended_next(&q, &zs).unwrap().id, "CLR-003");
        let after: Vec<OpenItem> = q.iter().filter(|i| i.id != "CLR-003").cloned().collect();
        assert_eq!(recommended_next(&after, &zs).unwrap().id, "CLR-004");
        // Security/Product/other-dev items never eligible.
        let eligible = eligible_items(&q, &zs);
        assert!(eligible.iter().all(|i| !matches!(i.id.as_str(), "CLR-001" | "CLR-002" | "CLR-005")));
    }

    #[test]
    fn group_membership_opens_items() {
        let u = CurrentUser::new("Taylor", vec!["QA".into()]);
        let q = vec![item("CLR-010", Priority::High, "Development", Some("QA"))];
        assert_eq!(recommended_next(&q, &u).unwrap().id, "CLR-010");
    }

    #[test]
    fn ownership_gaps_are_hidden_from_interview() {
        let mut o = item("CLR-020", Priority::Blocking, "Data Gov", None);
        o.kind = ItemKind::Ownership;
        let q = vec![o, item("CLR-004", Priority::High, "General", None)];
        let zs = zach();
        assert_eq!(recommended_next(&q, &zs).unwrap().id, "CLR-004");
    }

    #[test]
    fn unassigned_non_general_never_becomes_eligible() {
        let zs = zach();
        let q = vec![item("CLR-030", Priority::Blocking, "InfoSec", None)];
        assert!(recommended_next(&q, &zs).is_none());
    }
}
