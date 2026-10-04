//! Automatic Ownership-item synthesis (SPECIFICATION.md §8).
//!
//! If an open item carries a category that has no configured owner, the
//! planner creates (once per category) an Ownership item so the gap is
//! visible and cannot silently rot. Synthesis is deterministic and runs in
//! the apply step AFTER agent-driven mutations, so the agent never needs to
//! manufacture these itself.

mod resolution;
pub use resolution::resolve_assigned_gaps;

use crate::domain::{CategoryOwners, ItemKind, OpenItem, Priority, Stakeholders};

/// Scan the queue; return synthesized items for owner-less categories.
///
/// Suppression rules:
/// * a category WITH a configured owner produces nothing
/// * a category that ALREADY has an open Ownership item produces nothing
/// * `General` never produces one (it has no single owner concept)
pub fn synthesize_missing_owners(queue: &[OpenItem], stakeholders: &Stakeholders) -> Vec<OpenItem> {
    let mut out: Vec<OpenItem> = Vec::new();
    let mut seen_categories: Vec<String> = Vec::new();

    for item in queue {
        if item.category.eq_ignore_ascii_case("general") {
            continue;
        }
        let cat_key = item.category.to_ascii_lowercase();
        if seen_categories.iter().any(|s| s == &cat_key) {
            continue;
        }
        seen_categories.push(cat_key.clone());

        if stakeholders.owner_exists(&item.category) {
            continue;
        }
        let already_covered = queue.iter().any(|existing| {
            existing.is_ownership_gap() && existing.category.eq_ignore_ascii_case(&item.category)
        });
        if already_covered {
            continue;
        }

        let existing_entry: Option<&CategoryOwners> = stakeholders.find(&item.category);
        let blank = existing_entry.map_or(
            "No stakeholder mapping exists for this category.",
            |_| "The category is configured but has no members listed.",
        );
        let mut gap = OpenItem::new(
            format!(
                "OWN-PLACEHOLDER-{}",
                cat_key.chars().take(8).collect::<String>().to_uppercase()
            ),
            Priority::High,
            ItemKind::Ownership,
            item.category.clone(),
            None,
            format!("Category “{}” has no assigned stakeholder.", item.category),
            format!(
                "Automatically created: another open item ({}) requires “{}”, but {blank}",
                item.id, item.category
            ),
        );
        gap.conversation_id = Some(format!(
            "ownership:{}:{}",
            item.id,
            cat_key
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
        // Placeholder display IDs must also distinguish categories sharing a prefix.
        gap.id = gap.conversation_id.clone().unwrap();
        out.push(gap);
    }
    out
}

/// A newly raised gap must not reuse the conversation of a completed gap.
pub fn synthesize_for_state(state: &crate::core::state::PlannerState) -> Vec<OpenItem> {
    let mut gaps = synthesize_missing_owners(&state.items, &state.config.stakeholders);
    for gap in &mut gaps {
        let base = gap.conversation_key().to_owned();
        let mut key = base.clone();
        let mut generation = 1;
        while state
            .resolved_items
            .iter()
            .any(|old| old.conversation_key() == key)
        {
            generation += 1;
            key = format!("{base}:{generation}");
        }
        gap.id = key.clone();
        gap.conversation_id = Some(key);
    }
    gaps
}

/// Give synthesized items their final CLR ids, continuing from existing
/// numbering and avoiding clashes with this turn's agent-added items.
pub fn assign_ids(out: &mut [OpenItem], taken: impl IntoIterator<Item = String>) {
    let mut pool: Vec<String> = out.iter().map(|i| i.id.clone()).chain(taken).collect();
    for item in out.iter_mut() {
        let id = crate::core::ids::next_free(pool.iter().cloned());
        pool.push(id.clone());
        item.id = id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CategoryOwners, ItemKind, Priority};

    fn stakes() -> Stakeholders {
        Stakeholders::new(vec![CategoryOwners::new("Security", vec!["Morgan".into()])])
    }

    fn q(id: &str, cat: &str) -> OpenItem {
        OpenItem::new(
            id.into(),
            Priority::High,
            ItemKind::Question,
            cat.into(),
            Some("someone".into()),
            format!("q {id}"),
            "r".into(),
        )
    }

    #[test]
    fn synthesizes_only_unowned_categories_once_each() {
        let queue = vec![
            q("CLR-001", "Data Governance"),
            q("CLR-002", "DATA GOVERNANCE"),
            q("CLR-003", "Security"),
        ];
        let synth = synthesize_missing_owners(&queue, &stakes());
        assert_eq!(synth.len(), 1);
        assert!(synth[0].category.eq_ignore_ascii_case("Data Governance"));
        assert!(synth[0].reason.contains("CLR-001"));
        assert_eq!(
            synthesize_missing_owners(&queue, &Stakeholders::default()).len(),
            2
        );
    }

    #[test]
    fn suppressed_when_an_ownership_item_already_tracks_it() {
        let mut own = q("CLR-009", "Data Governance");
        own.kind = ItemKind::Ownership;
        let queue = vec![own, q("CLR-001", "Data Governance")];
        assert!(synthesize_missing_owners(&queue, &Stakeholders::default()).is_empty());
    }

    #[test]
    fn owned_and_general_produce_nothing() {
        let queue = vec![q("CLR-001", "Security"), q("CLR-002", "General")];
        assert!(synthesize_missing_owners(&queue, &stakes()).is_empty());
    }

    #[test]
    fn assign_ids_continues_numbering_without_clashes() {
        let mut synth =
            synthesize_missing_owners(&[q("CLR-004", "Risk")], &Stakeholders::default());
        assign_ids(&mut synth, ["CLR-005".to_string()]);
        assert_eq!(synth[0].id, "CLR-006");
    }
}
