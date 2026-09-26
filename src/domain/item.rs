//! Open-item model (SPECIFICATION.md §5–§9).
//!
//! An open item is a single unresolved planning issue. It is `Open` while it
//! exists in `.kool-ade-packet/planning/open-items.md` and ceases to exist once resolved
//! (resolution history is preserved by git, not by this model).

use serde::{Deserialize, Serialize};

mod open_item;
pub use open_item::OpenItem;

/// Priority drives queue ordering and the panel badges (higher urgency first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// The specification cannot meaningfully progress without an answer.
    Blocking,
    /// The answer could significantly change the specification or architecture.
    High,
    /// The answer matters but does not currently prevent progress.
    Normal,
}

impl Priority {
    /// Lower rank sorts first in the queue (Blocking before High before Normal).
    pub const fn rank(self) -> u8 {
        match self {
            Self::Blocking => 0,
            Self::High => 1,
            Self::Normal => 2,
        }
    }

    /// Badge label used in the right-hand panel.
    pub const fn badge(self) -> &'static str {
        match self {
            Self::Blocking => "BLOCKING",
            Self::High => "HIGH",
            Self::Normal => "NORMAL",
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Blocking => "Blocking",
            Self::High => "High",
            Self::Normal => "Normal",
        })
    }
}

/// Loose parser accepting the casing styles the harness tends to emit.
impl Priority {
    pub fn parse_i(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "blocking" | "blocker" | "critical" => Some(Self::Blocking),
            "high" => Some(Self::High),
            "normal" | "medium" | "mid" => Some(Self::Normal),
            _ => None,
        }
    }
}

/// The four item kinds permitted in the MVP (SPECIFICATION.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Information the agent cannot determine from the repository or docs.
    Question,
    /// Something in the current specification could reasonably mean several things.
    Ambiguity,
    /// Something the agent assumes but believes should be confirmed.
    Assumption,
    /// The item needs a stakeholder category that has no configured owner.
    Ownership,
}

impl ItemKind {
    pub fn parse_i(input: &str) -> Option<Self> {
        let folded: String = input
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        match folded.as_str() {
            "question" => Some(Self::Question),
            "ambiguity" | "ambiguous" => Some(Self::Ambiguity),
            "assumption" | "assumed" => Some(Self::Assumption),
            "ownership" | "owner" | "ownershipgap" => Some(Self::Ownership),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Question => "Question",
            Self::Ambiguity => "Ambiguity",
            Self::Assumption => "Assumption",
            Self::Ownership => "Ownership",
        }
    }
}

impl std::fmt::Display for ItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Item lifecycle. Only `Open` items are persisted; the enum mirrors the
/// model described in §5 (an item is either Open or Resolved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus {
    Open,
    Resolved,
}

impl ItemStatus {
    pub fn parse_i(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "resolved" | "closed" => Some(Self::Resolved),
            _ => None,
        }
    }
}

/// Who may settle an open item; independent of urgency and routing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Authority {
    Agent,
    Review,
    #[default]
    Human,
}
impl Authority {
    pub fn parse_i(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "agent" => Some(Self::Agent),
            "review" => Some(Self::Review),
            "human" => Some(Self::Human),
            _ => None,
        }
    }
}
impl std::fmt::Display for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Agent => "Agent",
            Self::Review => "Review",
            Self::Human => "Human",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_ordering_and_badges() {
        assert_eq!(Priority::rank_list(), [0u8, 1, 2]);
        assert_eq!(Priority::Blocking.badge(), "BLOCKING");
        assert_eq!(format!("{}", Priority::High), "High");
    }

    impl Priority {
        fn rank_list() -> [u8; 3] {
            [
                Self::Blocking.rank(),
                Self::High.rank(),
                Self::Normal.rank(),
            ]
        }
    }

    #[test]
    fn loose_parsers_accept_model_casing_variants() {
        assert_eq!(Priority::parse_i("BLOCKING"), Some(Priority::Blocking));
        assert_eq!(Priority::parse_i("high"), Some(Priority::High));
        assert_eq!(Priority::parse_i("Medium"), Some(Priority::Normal));
        assert_eq!(Priority::parse_i("urgent-ish"), None);
        assert_eq!(ItemKind::parse_i("Ambiguity"), Some(ItemKind::Ambiguity));
        assert_eq!(
            ItemKind::parse_i("ownership-gap"),
            Some(ItemKind::Ownership)
        );
        assert_eq!(ItemKind::parse_i("risk"), None);
        assert_eq!(ItemStatus::parse_i("CLOSED"), Some(ItemStatus::Resolved));
    }

    #[test]
    fn summary_collapses_whitespace() {
        let item = OpenItem::new(
            "CLR-001".into(),
            Priority::High,
            ItemKind::Question,
            "Product".into(),
            Some("Zach".into()),
            "what\n happens\n on conflict?".into(),
            "demo".into(),
        );
        assert!(item.summary().starts_with("CLR-001"));
        assert!(!item.summary().contains('\n'));
    }
}
