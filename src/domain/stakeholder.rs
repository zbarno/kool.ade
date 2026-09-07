//! Stakeholder/ownership configuration (SPECIFICATION.md §4, §7–§8).
//!
//! The configuration is a plain Markdown file (`.planner/config.md`). This
//! module owns the *in-memory* representation; Markdown round-tripping lives
//! in `crate::artifacts::config_io`.

/// A category name and the people/groups responsible for it.
///
/// `members` holds display strings: either a person ("Morgan") or a group
/// ("Product Team"). The distinction is semantic, not syntactic — routing
/// compares them against the current user's name and group memberships.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CategoryOwners {
    pub name: String,
    pub members: Vec<String>,
}

impl CategoryOwners {
    pub fn new(name: impl Into<String>, members: Vec<String>) -> Self {
        Self {
            name: name.into(),
            members,
        }
    }

    pub const fn has_owner(&self) -> bool {
        !self.members.is_empty()
    }
}

/// Ordered collection of category→owner mappings. Insertion order is
/// preserved because it is reflected verbatim in the serialized config.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stakeholders {
    pub entries: Vec<CategoryOwners>,
}

impl Stakeholders {
    pub fn new(entries: Vec<CategoryOwners>) -> Self {
        Self { entries }
    }

    /// Case-insensitive lookup of a category mapping.
    pub fn find(&self, category: &str) -> Option<&CategoryOwners> {
        let needle = category.trim().to_ascii_lowercase();
        self.entries.iter().find(|e| e.name.to_ascii_lowercase() == needle)
    }

    /// True when a category currently has at least one configured owner.
    /// Used to decide whether an auto Ownership item is warranted (§8).
    pub fn owner_exists(&self, category: &str) -> bool {
        self.find(category).is_some_and(CategoryOwners::has_owner)
    }

    /// Upsert a category mapping, preserving position when present.
    pub fn upsert(&mut self, entry: CategoryOwners) {
        if let Some(slot) = self.entries.iter_mut().find(|e| {
            e.name.to_ascii_lowercase() == entry.name.to_ascii_lowercase()
        }) {
            // Existing spelling wins; membership is refreshed.
            slot.members = entry.members;
        } else {
            self.entries.push(entry);
        }
    }

    pub fn iter_categories(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }
}

/// Categories the MVP ships with (SPECIFICATION.md §7). Used only to seed a
/// freshly-initialized config so teams see where to write owners.
pub const DEFAULT_CATEGORIES: &[&str] = &[
    "General",
    "Product",
    "Development",
    "QA",
    "InfoSec",
    "UX",
    "Operations",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_case_insensitive_and_ordered() {
        let h = Stakeholders::new(vec![
            CategoryOwners::new("product", vec!["Zach".into()]),
            CategoryOwners::new("qa", Vec::new()),
        ]);
        assert!(h.owner_exists("PRODUCT"));
        assert!(!h.owner_exists("QA"));
        assert!(!h.owner_exists("Ops"));
        let cats: Vec<&str> = h.iter_categories().collect();
        assert_eq!(cats, vec!["product", "qa"]);
    }

    #[test]
    fn upsert_preserves_position_and_updates_members() {
        let mut h = Stakeholders::new(vec![
            CategoryOwners::new("alpha", vec!["A".into()]),
            CategoryOwners::new("beta", vec!["B".into()]),
        ]);
        h.upsert(CategoryOwners::new("ALPHA", vec!["C".into()]));
        h.upsert(CategoryOwners::new("gamma", vec![]));
        let names: Vec<&str> = h.iter_categories().collect();
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
        assert_eq!(h.find("Alpha").unwrap().members, vec!["C".to_string()]);
    }
}
