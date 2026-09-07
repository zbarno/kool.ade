//! The identity of the person sitting at the keyboard (SPECIFICATION.md §10 step 2,
//! §11). Question eligibility is computed against this value.

/// Who is interacting with the planner right now.
///
/// `groups` are membership labels (team names, roles). An open item is
/// *eligible* for this user when it is assigned to the user by name, assigned
/// to one of the user's groups (matched against stakeholder members), or
/// categorized `General`. See `crate::core::routing`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CurrentUser {
    /// Display name, e.g. "Zach".
    pub name: String,
    /// Group/team memberships, e.g. ["Development", "Architecture"].
    pub groups: Vec<String>,
}

impl CurrentUser {
    pub fn new(name: impl Into<String>, groups: Vec<String>) -> Self {
        Self {
            name: name.into(),
            groups,
        }
    }

    pub fn is_set(&self) -> bool {
        !self.name.trim().is_empty()
    }

    /// Match a free-form assignment string against this user's identity.
    /// Returns the match flavor so the UI can explain *why* an item routes here.
    pub fn matches_assignment(&self, assigned_to: &str) -> AssignmentMatch {
        let a = assigned_to.trim();
        if a.eq_ignore_ascii_case(&self.name.trim()) {
            return AssignmentMatch::ByName;
        }
        if self
            .groups
            .iter()
            .any(|g| g.trim().eq_ignore_ascii_case(a))
        {
            return AssignmentMatch::ByGroup;
        }
        AssignmentMatch::Nobody
    }
}

/// Outcome of comparing an assignment against the current user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentMatch {
    /// Assigned to the user's name directly.
    ByName,
    /// Assigned to a group the user belongs to.
    ByGroup,
    Nobody,
}

/// Canonical label for "categorized General → everyone is eligible".
pub const GENERAL_CATEGORY: &str = "General";

/// Case-insensitive equality on the General category.
pub fn is_general_category(category: &str) -> bool {
    category.trim().eq_ignore_ascii_case(GENERAL_CATEGORY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_matching_flavors() {
        let u = CurrentUser::new("Zach", vec!["Development".into(), "Architecture".into()]);
        assert_eq!(u.matches_assignment("zach"), AssignmentMatch::ByName);
        assert_eq!(u.matches_assignment("ARCHITECTURE"), AssignmentMatch::ByGroup);
        assert_eq!(u.matches_assignment("Morgan"), AssignmentMatch::Nobody);
        assert!(u.is_set());
        assert!(!CurrentUser::default().is_set());
    }

    #[test]
    fn general_category_is_case_insensitive() {
        assert!(is_general_category("general "));
        assert!(!is_general_category("General-ish"));
    }
}
