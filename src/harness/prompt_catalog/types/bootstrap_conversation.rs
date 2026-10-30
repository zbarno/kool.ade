use serde::Serialize;

const MAX_TASK_TITLE_BYTES: usize = 512;
const MAX_FACTS: usize = 32;
const MAX_FACT_SUMMARY_BYTES: usize = 2 * 1024;
const MAX_HISTORY_MESSAGES: usize = 24;
const MAX_HISTORY_TEXT_BYTES: usize = 4 * 1024;
const MAX_CURRENT_MESSAGE_BYTES: usize = 8 * 1024;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapPlatformV1 {
    Linux,
    #[serde(rename = "macos")]
    MacOs,
    Windows,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapTaskStatusV1 {
    ActionRequired,
    InProgress,
    Blocked,
    Verified,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapFactDomainV1 {
    Onboarding,
    OperatingSystem,
    Provider,
    Authentication,
    ModelAccess,
    Docker,
    Git,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapFactStatusV1 {
    Ready,
    Missing,
    Unknown,
    Stale,
    Failed,
    Unsupported,
    Blocked,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapFactSourceV1 {
    Probe,
    Controller,
    UserReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootstrapTaskFactV1 {
    pub domain: BootstrapFactDomainV1,
    pub status: BootstrapFactStatusV1,
    pub source: BootstrapFactSourceV1,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapConversationSpeakerV1 {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapConversationSourceV1 {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootstrapAttributedTextV1 {
    pub source: BootstrapConversationSourceV1,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootstrapConversationMessageV1 {
    pub speaker: BootstrapConversationSpeakerV1,
    pub content: BootstrapAttributedTextV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootstrapConversationPayloadV1 {
    pub platform: BootstrapPlatformV1,
    pub task_title: String,
    pub task_status: BootstrapTaskStatusV1,
    pub facts: Vec<BootstrapTaskFactV1>,
    pub history: Vec<BootstrapConversationMessageV1>,
    pub current_message: BootstrapAttributedTextV1,
}

impl BootstrapConversationPayloadV1 {
    pub(super) fn validate(&self) -> Result<(), String> {
        let valid_text = |text: &str, max: usize| {
            !text.trim().is_empty() && text.len() <= max && !text.contains('\0')
        };
        if !valid_text(&self.task_title, MAX_TASK_TITLE_BYTES)
            || self.facts.len() > MAX_FACTS
            || self
                .facts
                .iter()
                .any(|fact| !valid_text(&fact.summary, MAX_FACT_SUMMARY_BYTES))
            || self.history.len() > MAX_HISTORY_MESSAGES
            || self.history.iter().any(|message| {
                let source_matches_speaker = matches!(
                    (message.speaker, message.content.source),
                    (
                        BootstrapConversationSpeakerV1::User,
                        BootstrapConversationSourceV1::User
                    ) | (
                        BootstrapConversationSpeakerV1::Assistant,
                        BootstrapConversationSourceV1::Assistant
                    )
                );
                !source_matches_speaker
                    || !valid_text(&message.content.text, MAX_HISTORY_TEXT_BYTES)
            })
            || self.current_message.source != BootstrapConversationSourceV1::User
            || !valid_text(&self.current_message.text, MAX_CURRENT_MESSAGE_BYTES)
        {
            return Err("bootstrap conversation payload contains invalid or oversized data".into());
        }
        if serde_json::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_PAYLOAD_BYTES
        {
            return Err(format!(
                "bootstrap conversation payload exceeds {MAX_PAYLOAD_BYTES} bytes"
            ));
        }
        Ok(())
    }
}
