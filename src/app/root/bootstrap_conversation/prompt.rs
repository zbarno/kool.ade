use crate::{
    harness::prompt_catalog::{
        BootstrapAttributedTextV1, BootstrapConversationMessageV1, BootstrapConversationPayloadV1,
        BootstrapConversationSourceV1, BootstrapConversationSpeakerV1, BootstrapFactDomainV1,
        BootstrapFactSourceV1, BootstrapFactStatusV1, BootstrapPlatformV1, BootstrapTaskFactV1,
        BootstrapTaskStatusV1, RoleInputV1, render,
    },
    harness::{ExecutionMode, LiveProgress, PlanningRequest},
    persistence::setup_tasks::{LocalTask, State},
};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc},
};

pub(super) fn render_request(
    task: &LocalTask,
    history: &[crate::domain::ChatMessage],
    current_message: &crate::domain::ChatMessage,
    host: crate::app::setup_attention::catalog::HostKind,
    private_working_directory: PathBuf,
    cancel: Arc<AtomicBool>,
) -> Result<PlanningRequest, String> {
    let payload = BootstrapConversationPayloadV1 {
        platform: platform(host),
        task_title: clip(&task.title, 512),
        task_status: task_status(task.state),
        facts: facts(task),
        history: recent_history(history),
        current_message: BootstrapAttributedTextV1 {
            source: BootstrapConversationSourceV1::User,
            text: clip(&current_message.text, 8 * 1024),
        },
    };
    let rendered = render(&RoleInputV1::bootstrap_conversation(payload))?;
    let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
    Ok(PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: None,
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        prompt_manifest: Some(rendered.manifest),
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: private_working_directory,
        runtime_config_source: None,
        prompt_body: rendered.user,
        system_instructions: rendered.system,
        timeout: crate::core::turn::configured_turn_timeout(),
        progress_tx,
        cancel,
        review_identity: None,
    })
}

fn platform(host: crate::app::setup_attention::catalog::HostKind) -> BootstrapPlatformV1 {
    use crate::app::setup_attention::catalog::HostKind;
    match host {
        HostKind::Linux => BootstrapPlatformV1::Linux,
        HostKind::MacOs => BootstrapPlatformV1::MacOs,
        HostKind::Windows => BootstrapPlatformV1::Windows,
        HostKind::Unsupported => BootstrapPlatformV1::Other,
    }
}

fn task_status(state: State) -> BootstrapTaskStatusV1 {
    match state {
        State::Verified => BootstrapTaskStatusV1::Verified,
        State::InProgress | State::NeedsApproval | State::AwaitingVerification => {
            BootstrapTaskStatusV1::InProgress
        }
        State::BlockedByPrerequisite => BootstrapTaskStatusV1::Blocked,
        State::Unsupported | State::Superseded => BootstrapTaskStatusV1::Unknown,
        _ => BootstrapTaskStatusV1::ActionRequired,
    }
}

fn facts(task: &LocalTask) -> Vec<BootstrapTaskFactV1> {
    let domain = if task.id.starts_with("setup:cli:") {
        BootstrapFactDomainV1::Provider
    } else if task.id.starts_with("setup:git:") {
        BootstrapFactDomainV1::Git
    } else if task.id.starts_with("setup:network:") {
        BootstrapFactDomainV1::Network
    } else if task.id.starts_with("setup:os:") {
        BootstrapFactDomainV1::OperatingSystem
    } else {
        BootstrapFactDomainV1::Docker
    };
    let status = match task.state {
        State::Verified => BootstrapFactStatusV1::Ready,
        State::Unsupported => BootstrapFactStatusV1::Unsupported,
        State::BlockedByPrerequisite => BootstrapFactStatusV1::Blocked,
        State::Failed | State::Interrupted | State::Cancelled => BootstrapFactStatusV1::Failed,
        State::Superseded => BootstrapFactStatusV1::Stale,
        _ => BootstrapFactStatusV1::Unknown,
    };
    let mut summary = format!(
        "Controller state: {:?}. {} Next step: {}",
        task.state,
        clip(&task.explanation, 700),
        clip(&task.next_step, 700)
    );
    if !task.blocked_by.is_empty() {
        summary.push_str(&format!(
            " Waiting for {} setup prerequisite(s).",
            task.blocked_by.len().min(16)
        ));
    }
    let mut facts = vec![BootstrapTaskFactV1 {
        domain,
        status,
        source: BootstrapFactSourceV1::Controller,
        summary: clip(&summary, 2 * 1024),
    }];
    if let Some(result) = &task.verified_result {
        facts.push(BootstrapTaskFactV1 {
            domain,
            status: BootstrapFactStatusV1::Ready,
            source: BootstrapFactSourceV1::Probe,
            summary: clip(
                &format!(
                    "The application verified evidence domain {} at scan generation {}.",
                    result.evidence_domain, result.scan_generation
                ),
                2 * 1024,
            ),
        });
    }
    facts
}

fn recent_history(messages: &[crate::domain::ChatMessage]) -> Vec<BootstrapConversationMessageV1> {
    let mut selected = Vec::new();
    let mut remaining = 4 * 1024;
    for message in messages.iter().rev() {
        let (speaker, source) = match message.role {
            crate::domain::ChatRole::User => (
                BootstrapConversationSpeakerV1::User,
                BootstrapConversationSourceV1::User,
            ),
            crate::domain::ChatRole::Agent => (
                BootstrapConversationSpeakerV1::Assistant,
                BootstrapConversationSourceV1::Assistant,
            ),
            crate::domain::ChatRole::System => continue,
        };
        if selected.len() >= 24 || remaining == 0 {
            break;
        }
        let text = clip(&message.text, remaining.min(1024));
        if text.is_empty() {
            continue;
        }
        remaining = remaining.saturating_sub(text.len());
        selected.push(BootstrapConversationMessageV1 {
            speaker,
            content: BootstrapAttributedTextV1 { source, text },
        });
    }
    selected.reverse();
    selected
}

fn clip(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut boundary = max_bytes;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text[..boundary].to_owned()
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
