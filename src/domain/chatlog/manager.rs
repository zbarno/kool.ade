use serde::{Deserialize, Serialize};
use sha2::Digest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerEventCheckpointV1 {
    pub cursor: u64,
    pub events: Vec<ManagerInputEventV1>,
    pub sha256: String,
    #[serde(default)]
    pub route: Option<crate::harness::CapabilityRefreshBindingV1>,
    #[serde(default)]
    pub baseline_review_id: Option<String>,
    #[serde(default)]
    pub baseline_repository_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerInputEventV1 {
    pub sequence: u64,
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerRunOutcomeV1 {
    Queued,
    Started,
    Completed,
    Failed,
    TimedOut,
    Retried,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagerRunEventV1 {
    pub run_id: String,
    pub sequence: u64,
    pub outcome: ManagerRunOutcomeV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<ManagerEventCheckpointV1>,
    pub checkpoint_cursor: u64,
    pub checkpoint_sha256: String,
    #[serde(default)]
    pub retry_after_unix_ms: Option<u64>,
}

impl ManagerEventCheckpointV1 {
    pub fn from_messages(
        messages: &[String],
        route: Option<crate::harness::CapabilityRefreshBindingV1>,
        baseline_review_id: Option<String>,
        baseline_repository_id: Option<String>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !messages.is_empty() && messages.len() <= 128,
            "Manager checkpoint event count is invalid"
        );
        let events = messages
            .iter()
            .enumerate()
            .map(|(index, text)| {
                let sequence = index as u64 + 1;
                let id = format!(
                    "{:x}",
                    sha2::Sha256::digest(format!("{sequence}\0{text}").as_bytes())
                );
                ManagerInputEventV1 {
                    sequence,
                    id,
                    text: text.clone(),
                }
            })
            .collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&events)?;
        anyhow::ensure!(
            bytes.len() <= 256 * 1024,
            "Manager checkpoint exceeds its size limit"
        );
        let checkpoint = Self {
            cursor: events.len() as u64,
            events,
            sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
            route,
            baseline_review_id,
            baseline_repository_id,
        };
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.events.is_empty()
                && self.events.len() <= 128
                && self.cursor == self.events.len() as u64,
            "Manager checkpoint cursor is invalid"
        );
        anyhow::ensure!(
            self.route.as_ref().is_none_or(|route| route.is_valid()),
            "Manager checkpoint route binding is invalid"
        );
        anyhow::ensure!(
            self.baseline_review_id
                .as_deref()
                .is_none_or(|value| !value.is_empty() && value.len() <= 256)
                && self
                    .baseline_repository_id
                    .as_deref()
                    .is_none_or(|value| !value.is_empty() && value.len() <= 256),
            "Manager checkpoint baseline identity is invalid"
        );
        let mut total = 0usize;
        for (index, event) in self.events.iter().enumerate() {
            anyhow::ensure!(
                event.sequence == index as u64 + 1 && event.text.len() <= 4096,
                "Manager checkpoint event identity is invalid"
            );
            let expected = format!(
                "{:x}",
                sha2::Sha256::digest(format!("{}\0{}", event.sequence, event.text).as_bytes())
            );
            anyhow::ensure!(
                event.id == expected,
                "Manager checkpoint event hash is invalid"
            );
            total = total.saturating_add(event.text.len());
        }
        anyhow::ensure!(
            total <= 256 * 1024,
            "Manager checkpoint exceeds its size limit"
        );
        let bytes = serde_json::to_vec(&self.events)?;
        anyhow::ensure!(
            self.sha256 == format!("{:x}", sha2::Sha256::digest(&bytes)),
            "Manager checkpoint digest is invalid"
        );
        Ok(())
    }
}

impl ManagerRunEventV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        let id = uuid::Uuid::parse_str(&self.run_id)?;
        anyhow::ensure!(id.hyphenated().to_string() == self.run_id && self.sequence > 0);
        if self.outcome == ManagerRunOutcomeV1::Started
            || (self.outcome == ManagerRunOutcomeV1::Queued && self.sequence == 1)
        {
            let checkpoint = self
                .checkpoint
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Manager pending event has no checkpoint"))?;
            checkpoint.validate()?;
            anyhow::ensure!(self.checkpoint_cursor == checkpoint.cursor);
            anyhow::ensure!(self.checkpoint_sha256 == checkpoint.sha256);
        } else {
            anyhow::ensure!(
                self.checkpoint.is_none()
                    && self.checkpoint_cursor > 0
                    && self.checkpoint_sha256.len() == 64
                    && self
                        .checkpoint_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
            );
        }
        anyhow::ensure!(
            self.retry_after_unix_ms.is_none_or(|deadline| deadline > 0),
            "Manager retry deadline is invalid"
        );
        anyhow::ensure!(
            self.retry_after_unix_ms.is_none() || self.outcome == ManagerRunOutcomeV1::Queued,
            "Only queued Manager events may defer retry"
        );
        Ok(())
    }

    pub fn validate_after(&self, previous: Option<&Self>) -> anyhow::Result<()> {
        self.validate()?;
        match previous {
            Some(previous) => {
                anyhow::ensure!(previous.run_id == self.run_id);
                anyhow::ensure!(self.sequence == previous.sequence.saturating_add(1));
                anyhow::ensure!(previous.checkpoint_sha256 == self.checkpoint_sha256);
                anyhow::ensure!(previous.checkpoint_cursor == self.checkpoint_cursor);
            }
            None => anyhow::ensure!(self.sequence == 1),
        }
        Ok(())
    }
}

pub fn checkpoint_attempts(history: &[super::ChatMessage], digest: &str) -> usize {
    let mut attempts = 0;
    let mut previous = None;
    for event in history
        .iter()
        .filter_map(|message| message.manager_run_event.as_ref())
        .filter(|event| event.checkpoint_sha256 == digest)
    {
        match event.outcome {
            ManagerRunOutcomeV1::Started => {
                if matches!(
                    previous,
                    Some(
                        ManagerRunOutcomeV1::Completed
                            | ManagerRunOutcomeV1::Failed
                            | ManagerRunOutcomeV1::TimedOut
                    )
                ) {
                    attempts = 0;
                }
                attempts += 1;
            }
            ManagerRunOutcomeV1::Completed => attempts = 0,
            ManagerRunOutcomeV1::Queued
            | ManagerRunOutcomeV1::Failed
            | ManagerRunOutcomeV1::TimedOut
            | ManagerRunOutcomeV1::Retried => {}
        }
        previous = Some(event.outcome);
    }
    attempts
}

#[cfg(test)]
mod tests;
