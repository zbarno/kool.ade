use crate::domain::{ItemKind, ItemStatus, OpenItem, Stakeholders};

/// Resolve ownership-gap items as soon as their category has an assigned owner.
/// The caller persists both queues in the same artifact transaction as the
/// stakeholder configuration update.
pub fn resolve_assigned_gaps(
    open: &mut Vec<OpenItem>,
    resolved: &mut Vec<OpenItem>,
    stakeholders: &Stakeholders,
) -> Vec<String> {
    let mut moved = Vec::new();
    let mut remaining = Vec::with_capacity(open.len());
    for mut item in std::mem::take(open) {
        let Some(owners) = stakeholders
            .owner_exists(&item.category)
            .then(|| stakeholders.find(&item.category))
            .flatten()
        else {
            remaining.push(item);
            continue;
        };
        if item.kind != ItemKind::Ownership || item.status != ItemStatus::Open {
            remaining.push(item);
            continue;
        }

        let owners = owners.members.join(", ");
        item.status = ItemStatus::Resolved;
        item.evidence.push_str(&format!(
            "\n\nResolution outcome: Ownership was assigned to {owners} in Stakeholders & ownership settings."
        ));
        resolved.retain(|existing| existing.id != item.id);
        moved.push(item.id.clone());
        resolved.push(item);
    }
    *open = remaining;
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CategoryOwners, ItemKind, Priority};

    fn item(id: &str, kind: ItemKind, category: &str) -> OpenItem {
        OpenItem::new(
            id.into(),
            Priority::High,
            kind,
            category.into(),
            None,
            format!("Who owns {category}?"),
            "Ownership is required for this category.".into(),
        )
    }

    #[test]
    fn assigned_owner_resolves_only_the_matching_ownership_gap() {
        let mut open = vec![
            item("CLR-001", ItemKind::Ownership, "Security"),
            item("CLR-002", ItemKind::Question, "Security"),
            item("CLR-003", ItemKind::Ownership, "Operations"),
        ];
        let mut resolved = Vec::new();
        let stakeholders = Stakeholders::new(vec![CategoryOwners::new(
            " security ",
            vec!["Morgan".into()],
        )]);

        let moved = resolve_assigned_gaps(&mut open, &mut resolved, &stakeholders);

        assert_eq!(moved, vec!["CLR-001"]);
        assert_eq!(
            open.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            ["CLR-002", "CLR-003"]
        );
        assert_eq!(resolved[0].status, ItemStatus::Resolved);
        assert!(resolved[0].evidence.contains("assigned to Morgan"));
    }
}
