//! The identity of the person sitting at the keyboard (SPECIFICATION.md §10 step 2,
//! §11). Question eligibility is computed against this value.

/// Who is interacting with the planner right now.
///
/// `groups` are membership labels (team names, roles). Question
/// eligibility against this seat obeys the D-14 routing law (approved
/// specification §8; details in `crate::core::routing`): an item is
/// poseable when (1) its category is the General broadcast, (2) it is
/// addressed to the user by name or one of the user's groups,
/// (3) its category is explicitly configured to the user — sole person
/// or owning group — or (4) its category has NO explicit owner, in which
/// case the seated (non-guest) git-identified operator inherits the
/// lane. Lanes claimed by other holders are never posed, even on direct
/// address.
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
        if a.eq_ignore_ascii_case(self.name.trim()) {
            return AssignmentMatch::ByName;
        }
        if self.groups.iter().any(|g| g.trim().eq_ignore_ascii_case(a)) {
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

/// How the seated operator's identity was derived (FR-13 / D-14 tier order,
/// verbatim from the approved specification §5):
/// `user.name` preferred, `user.email` fallback, the config's Current User
/// block as tertiary source, `(guest)` last resort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySource {
    /// Seated from the connected repository's `git config user.name`.
    GitUserName,
    /// Seated from `git config user.email` because `user.name` yielded nothing.
    GitUserEmail,
    /// Seated from the `## Current User` block in Packet's project config (fallback/override).
    ConfigBlock,
    /// No identity resolvable anywhere; the session runs as a neutral guest.
    Guest,
}

impl IdentitySource {
    /// UI-ready provenance text for the seated identity (settings card echo).
    pub fn label(&self) -> &'static str {
        match self {
            Self::GitUserName => "derived from git user.name",
            Self::GitUserEmail => "derived from git user.email (fallback)",
            Self::ConfigBlock => "from .kool-ade-packet/config/project.md override",
            Self::Guest => "no identity found — guest",
        }
    }
}

/// The seated operator plus how the seat was derived. Computed once per
/// `PlannerState::load` (connection and every resync) by [`resolve_identity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIdentity {
    pub user: CurrentUser,
    pub source: IdentitySource,
}

/// Trim a maybe-valued identifier tier; whitespace-only counts as absent.
fn trimmed_present(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

/// Pure composition of the FR-13 precedence ladder for the seated operator.
///
/// Tier order for the NAME: trimmed-non-empty `git_name` beats trimmed-
/// non-empty `git_email`, beats the config block's name, beats `(guest)`.
/// GROUPS always come exclusively from the config block (git has no notion
/// of groups): a git-won seat carries the config groups when present, else
/// none. Whitespace-only values at ANY tier count as absent, so descent is
/// purely lexical and can never error — every failure mode of the upstream
/// git probe simply reads as `None` here.
pub fn resolve_identity(
    git_name: Option<&str>,
    git_email: Option<&str>,
    config_user: Option<&CurrentUser>,
) -> ResolvedIdentity {
    let config_groups = config_user.map(|u| u.groups.clone()).unwrap_or_default();
    let config_name = config_user.and_then(|u| trimmed_present(Some(&u.name)));
    if let Some(name) = trimmed_present(git_name) {
        return ResolvedIdentity {
            user: CurrentUser::new(name, config_groups),
            source: IdentitySource::GitUserName,
        };
    }
    if let Some(email) = trimmed_present(git_email) {
        return ResolvedIdentity {
            user: CurrentUser::new(email, config_groups),
            source: IdentitySource::GitUserEmail,
        };
    }
    if let Some(name) = config_name {
        return ResolvedIdentity {
            user: CurrentUser::new(name, config_groups),
            source: IdentitySource::ConfigBlock,
        };
    }
    ResolvedIdentity {
        user: CurrentUser::new(GUEST_NAME, Vec::new()),
        source: IdentitySource::Guest,
    }
}

/// Neutral seat label when no identity can be derived (last FR-13 tier).
pub const GUEST_NAME: &str = "(guest)";

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
        assert_eq!(
            u.matches_assignment("ARCHITECTURE"),
            AssignmentMatch::ByGroup
        );
        assert_eq!(u.matches_assignment("Morgan"), AssignmentMatch::Nobody);
        assert!(u.is_set());
        assert!(!CurrentUser::default().is_set());
    }

    #[test]
    fn general_category_is_case_insensitive() {
        assert!(is_general_category("general "));
        assert!(!is_general_category("General-ish"));
    }

    #[test]
    fn resolve_identity_follows_the_fr13_precedence_matrix() {
        // 1. git user.name wins outright; config contributes its groups.
        let r = resolve_identity(
            Some("Ada Lovelace"),
            Some("ada@example.org"),
            Some(&CurrentUser::new("Bob", vec!["Dev".into(), "QA".into()])),
        );
        assert_eq!(r.user.name, "Ada Lovelace");
        assert_eq!(r.user.groups, vec!["Dev".to_string(), "QA".to_string()]);
        assert_eq!(r.source, IdentitySource::GitUserName);

        // 2. user.name absent → user.email wins; groups STILL from the config block.
        let r = resolve_identity(
            None,
            Some("eve@example.org"),
            Some(&CurrentUser::new("Dana", vec!["Ops".into()])),
        );
        assert_eq!(r.user.name, "eve@example.org");
        assert_eq!(r.user.groups, vec!["Ops".to_string()]);
        assert_eq!(r.source, IdentitySource::GitUserEmail);

        // 3. Whitespace-only tiers are SKIPPED, descending lexically.
        let r = resolve_identity(Some("  "), Some("zed@example.org"), None);
        assert_eq!(r.user.name, "zed@example.org");
        assert_eq!(r.user.groups, Vec::<String>::new());
        assert_eq!(r.source, IdentitySource::GitUserEmail);

        // 4. Git yields nothing → the config block seats the operator.
        let r = resolve_identity(
            None,
            None,
            Some(&CurrentUser::new(
                "Dana",
                vec!["Ops".into(), "Platform".into()],
            )),
        );
        assert_eq!(r.user.name, "Dana");
        assert_eq!(
            r.user.groups,
            vec!["Ops".to_string(), "Platform".to_string()]
        );
        assert_eq!(r.source, IdentitySource::ConfigBlock);

        // 5. Nothing anywhere → neutral guest, groups stripped.
        let r = resolve_identity(None, None, None);
        assert_eq!(r.user.name, GUEST_NAME);
        assert_eq!(r.user.groups, Vec::<String>::new());
        assert_eq!(r.source, IdentitySource::Guest);

        // 6. Empty string ≡ whitespace at a tier (both "absent"), and a
        //    whitespace-only config name descends to guest despite groups.
        let empty = resolve_identity(Some(""), Some("nully@example.org"), None);
        let ws = resolve_identity(Some("   "), Some("nully@example.org"), None);
        assert_eq!(empty, ws);
        let ghosted =
            resolve_identity(None, None, Some(&CurrentUser::new("   ", vec!["X".into()])));
        assert_eq!(ghosted.source, IdentitySource::Guest);
        assert_eq!(ghosted.user.name, GUEST_NAME);
    }

    #[test]
    fn identity_source_labels_read_as_provenance() {
        assert_eq!(
            IdentitySource::GitUserName.label(),
            "derived from git user.name"
        );
        assert_eq!(
            IdentitySource::GitUserEmail.label(),
            "derived from git user.email (fallback)"
        );
        assert_eq!(
            IdentitySource::ConfigBlock.label(),
            "from .kool-ade-packet/config/project.md override"
        );
        assert_eq!(IdentitySource::Guest.label(), "no identity found — guest");
    }
}
