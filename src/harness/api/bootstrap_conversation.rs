use serde::{Deserialize, Serialize};

const MAX_RESPONSE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapIntentOperationV1 {
    RecheckPrerequisites,
    PrepareLockedImage,
    OpenCodingTools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapImageProfileV1 {
    General,
    Rust,
    Node,
    Python,
    Go,
}

/// Closed response choices rendered by the app from its own setup facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapConversationReplyV1 {
    ExplainObservation,
    ExplainDocker,
    ExplainGit,
    ExplainManager,
    RepeatNextStep,
    AskWhatToExplain,
    AskWhatTaskShows,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootstrapIntentSuggestionV1 {
    pub operation: BootstrapIntentOperationV1,
    #[serde(default)]
    pub image_profile: Option<BootstrapImageProfileV1>,
}

impl BootstrapIntentSuggestionV1 {
    fn validate(&self) -> Result<(), String> {
        let image_profile_matches = matches!(
            (self.operation, self.image_profile),
            (BootstrapIntentOperationV1::PrepareLockedImage, Some(_))
                | (
                    BootstrapIntentOperationV1::RecheckPrerequisites
                        | BootstrapIntentOperationV1::OpenCodingTools,
                    None
                )
        );
        if image_profile_matches {
            Ok(())
        } else {
            Err("imageProfile is required only for prepare_locked_image".into())
        }
    }
}

/// Strict, non-executable output for one setup-assistant turn.
///
/// The model selects a reply kind. Kool.ad/e renders the user-facing text from
/// fixed templates and the current app-owned task summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootstrapConversationResponseV1 {
    pub schema_version: u8,
    pub reply: BootstrapConversationReplyV1,
    #[serde(default)]
    pub suggestion: Option<BootstrapIntentSuggestionV1>,
}

impl BootstrapConversationResponseV1 {
    /// Decode exactly one JSON object and validate all version and size bounds.
    pub fn parse_json(json: &str) -> Result<Self, String> {
        if json.len() > MAX_RESPONSE_BYTES {
            return Err(format!(
                "bootstrap conversation response exceeds {MAX_RESPONSE_BYTES} bytes"
            ));
        }
        let response: Self =
            serde_json::from_str(json).map_err(|error| format!("invalid response: {error}"))?;
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported BootstrapConversationResponseV1 schema version {}",
                self.schema_version
            ));
        }
        if let Some(suggestion) = &self.suggestion {
            suggestion.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "bootstrap_conversation/tests.rs"]
mod tests;
