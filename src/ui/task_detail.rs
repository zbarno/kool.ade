//! Read model and typed user commands for one task's detail surface.

use crate::domain::ChatMessage;

#[derive(Clone)]
pub struct ViewModel {
    pub implementation: Option<crate::core::implementation::Implementation>,
    pub implementation_active: bool,
    pub failure: Option<String>,
    pub failure_disposition: Option<crate::core::implementation::RecoveryDisposition>,
    pub attention: Option<crate::core::attention::View>,
    pub messages: Vec<ChatMessage>,
    pub progress: Option<crate::harness::LiveProgress>,
    pub conversation_active: bool,
    pub conversation_error: Option<String>,
    pub board_column: usize,
    pub can_start: bool,
    pub auto_build: bool,
    pub draft: String,
    pub activity_samples: Vec<(i64, u64)>,
    pub activity_active: bool,
    pub implementation_metrics: Option<crate::persistence::telemetry::ImplementationMetrics>,
    pub feature_metrics: Option<crate::persistence::telemetry::ImplementationMetrics>,
}

pub enum Command {
    UpdateDraft {
        ticket: String,
        draft: String,
    },
    SubmitReply {
        ticket: String,
        draft: String,
        decision: bool,
    },
    RetryChatSave,
    RetryExplanation {
        ticket: String,
        detail: String,
    },
    CancelTask {
        ticket: String,
    },
    StartOrResume {
        ticket: String,
    },
}
