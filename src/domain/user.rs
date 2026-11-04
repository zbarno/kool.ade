//! The local human identity for this Kool.ad/e installation.

/// Who is interacting with the planner right now.
///
/// Human input is routed to this local person. The identity does not represent
/// a team membership or shared task owner.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CurrentUser {
    /// Display name, e.g. "Zach".
    pub name: String,
}

impl CurrentUser {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn is_set(&self) -> bool {
        !self.name.trim().is_empty()
    }
}

/// How the local operator's identity was derived:
/// `user.name` preferred, `user.email` fallback, the config's Current User
/// block as tertiary source, `(guest)` last resort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySource {
    /// Derived from the connected repository's `git config user.name`.
    GitUserName,
    /// Derived from `git config user.email` because `user.name` yielded nothing.
    GitUserEmail,
    /// Derived from the local user setting.
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
            Self::ConfigBlock => "from local user setting",
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
/// Whitespace-only values at ANY tier count as absent, so descent is
/// purely lexical and can never error — every failure mode of the upstream
/// git probe simply reads as `None` here.
pub fn resolve_identity(
    git_name: Option<&str>,
    git_email: Option<&str>,
    config_user: Option<&CurrentUser>,
) -> ResolvedIdentity {
    let config_name = config_user.and_then(|u| trimmed_present(Some(&u.name)));
    if let Some(name) = trimmed_present(git_name) {
        return ResolvedIdentity {
            user: CurrentUser::new(name),
            source: IdentitySource::GitUserName,
        };
    }
    if let Some(email) = trimmed_present(git_email) {
        return ResolvedIdentity {
            user: CurrentUser::new(email),
            source: IdentitySource::GitUserEmail,
        };
    }
    if let Some(name) = config_name {
        return ResolvedIdentity {
            user: CurrentUser::new(name),
            source: IdentitySource::ConfigBlock,
        };
    }
    ResolvedIdentity {
        user: CurrentUser::new(GUEST_NAME),
        source: IdentitySource::Guest,
    }
}

/// Neutral seat label when no identity can be derived (last FR-13 tier).
pub const GUEST_NAME: &str = "(guest)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_resolution_prefers_git_then_local_setting_then_guest() {
        let git_name = resolve_identity(
            Some("Ada Lovelace"),
            Some("ada@example.invalid"),
            Some(&CurrentUser::new("Bob")),
        );
        assert_eq!(git_name.user.name, "Ada Lovelace");
        assert_eq!(git_name.source, IdentitySource::GitUserName);

        let email = resolve_identity(None, Some("ada@example.invalid"), None);
        assert_eq!(email.user.name, "ada@example.invalid");
        assert_eq!(email.source, IdentitySource::GitUserEmail);

        let local = resolve_identity(None, None, Some(&CurrentUser::new("Dana")));
        assert_eq!(local.user.name, "Dana");
        assert_eq!(local.source, IdentitySource::ConfigBlock);

        let guest = resolve_identity(None, None, None);
        assert_eq!(guest.user.name, GUEST_NAME);
        assert_eq!(guest.source, IdentitySource::Guest);
    }

    #[test]
    fn empty_names_fall_through_to_a_usable_identity() {
        let identity = resolve_identity(Some(" "), Some("eve@example.invalid"), None);
        assert_eq!(identity.user.name, "eve@example.invalid");
        assert!(CurrentUser::default().name.is_empty());
    }
}
