use serde::{Deserialize, Serialize};

mod metadata;
pub use metadata::ChangeMetadata;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Draft,
    Ready,
    Implementing,
    Reconciliation,
    Implemented,
    Abandoned,
}

impl ChangeStatus {
    pub const ALL: [Self; 6] = [
        Self::Draft,
        Self::Ready,
        Self::Implementing,
        Self::Reconciliation,
        Self::Implemented,
        Self::Abandoned,
    ];

    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::Implementing => "implementing",
            Self::Reconciliation => "reconciliation",
            Self::Implemented => "implemented",
            Self::Abandoned => "abandoned",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Ready => "Ready",
            Self::Implementing => "Implementing",
            Self::Reconciliation => "Reconciliation",
            Self::Implemented => "Implemented",
            Self::Abandoned => "Abandoned",
        }
    }

    pub fn parse_wire(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.wire_name() == value)
    }

    /// Strictly parse the legacy human-readable status, including supported
    /// explanatory suffixes, during migration only.
    pub fn parse_legacy(value: &str) -> anyhow::Result<Self> {
        let value = value.trim();
        for status in Self::ALL {
            if let Some(suffix) = value.strip_prefix(status.label()) {
                let suffix = suffix.trim_start();
                anyhow::ensure!(
                    suffix.is_empty()
                        || suffix.starts_with('—')
                        || suffix.starts_with('-')
                        || suffix.starts_with(':')
                        || suffix.starts_with('('),
                    "Unknown or ambiguous change status {value:?}"
                );
                return Ok(status);
            }
        }
        anyhow::bail!("Unknown or ambiguous change status {value:?}")
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        use ChangeStatus::*;
        self as u8 == next as u8
            || matches!(
                (self, next),
                (Draft, Ready | Abandoned)
                    | (Ready, Draft | Implementing | Abandoned)
                    | (
                        Implementing,
                        Ready | Reconciliation | Implemented | Abandoned
                    )
                    | (Reconciliation, Implementing | Implemented | Abandoned)
            )
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Implemented | Self::Abandoned)
    }

    pub const fn approval_eligible(self) -> bool {
        matches!(self, Self::Ready | Self::Implementing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ArtifactIdentity;

    #[test]
    fn structured_status_overrides_prose_and_renders_from_metadata() {
        for (status, prose) in [
            (ChangeStatus::Ready, "Draft"),
            (ChangeStatus::Draft, "Ready"),
        ] {
            let markdown = format!("# CHG-001: Search\n\n**Status:** {prose}\n");
            let markdown =
                ArtifactIdentity::preserve_markdown(&markdown, None, "CHG-001", "Search").unwrap();
            let identity = ArtifactIdentity::from_markdown(&markdown).unwrap().unwrap();
            let saved = ChangeMetadata::write_markdown(&markdown, &identity, status).unwrap();
            let metadata = ChangeMetadata::require_markdown(&saved).unwrap();
            assert_eq!(metadata.status, status);
            let conflicting = saved.replace(
                &format!("**Status:** {}", status.label()),
                &format!("**Status:** {prose}"),
            );
            assert_eq!(
                ChangeMetadata::require_markdown(&conflicting)
                    .unwrap()
                    .status,
                status
            );
            assert!(
                ChangeMetadata::render_status(&conflicting, metadata.status)
                    .unwrap()
                    .contains(&format!("**Status:** {}", status.label()))
            );
        }
    }

    #[test]
    fn legacy_status_parser_is_strict_and_wire_values_are_stable() {
        assert_eq!(
            ChangeStatus::parse_legacy("Ready — planning complete").unwrap(),
            ChangeStatus::Ready
        );
        assert_eq!(
            ChangeStatus::parse_wire("reconciliation"),
            Some(ChangeStatus::Reconciliation)
        );
        assert!(ChangeStatus::parse_legacy("Ready whenever").is_err());
        assert!(ChangeStatus::parse_legacy("Unknown").is_err());
    }

    #[test]
    fn lifecycle_transitions_reject_unsupported_shortcuts() {
        assert!(ChangeStatus::Draft.can_transition_to(ChangeStatus::Ready));
        assert!(ChangeStatus::Ready.can_transition_to(ChangeStatus::Implementing));
        assert!(!ChangeStatus::Draft.can_transition_to(ChangeStatus::Implemented));
        assert!(!ChangeStatus::Implemented.can_transition_to(ChangeStatus::Draft));
    }

    #[test]
    fn controlled_write_renders_status_when_model_omits_the_line() {
        let original = "# CHG-001: Search\n\n## Intent\n\nPersist searches.\n";
        let identified =
            ArtifactIdentity::preserve_markdown(original, None, "CHG-001", "Search").unwrap();
        let identity = ArtifactIdentity::from_markdown(&identified)
            .unwrap()
            .unwrap();
        let saved =
            ChangeMetadata::write_markdown(&identified, &identity, ChangeStatus::Ready).unwrap();
        assert!(saved.contains("**Status:** Ready"));
        assert_eq!(
            ChangeMetadata::require_markdown(&saved).unwrap().status,
            ChangeStatus::Ready
        );
    }
}
