//! Conversation context is distinct from setup authority: wizard guidance is
//! never inserted into SetupTaskStore or represented as a verified task.
use super::*;
use crate::app::welcome::onboarding::{Onboarding, Step};
use crate::harness::prompt_catalog::{
    BootstrapFactDomainV1, BootstrapFactSourceV1, BootstrapFactStatusV1, BootstrapTaskFactV1,
    BootstrapTaskStatusV1,
};

pub(crate) const ONBOARDING_CHAT: &str = "setup:onboarding";

#[derive(Clone)]
pub(crate) enum Context {
    Task(Box<LocalTask>),
    Onboarding {
        step: Step,
        revision: u64,
        tasks: Vec<LocalTask>,
    },
}

impl Context {
    pub(crate) fn onboarding(session: &Onboarding, tasks: &[LocalTask]) -> Self {
        let mut tasks = tasks.to_vec();
        tasks.retain(|task| task.state != crate::persistence::setup_tasks::State::Superseded);
        tasks.sort_by_key(|task| {
            (
                session.active_setup_task_id.as_deref() != Some(task.id.as_str()),
                task.state == crate::persistence::setup_tasks::State::Verified,
            )
        });
        Self::Onboarding {
            step: session.step,
            revision: session.revision,
            tasks: tasks.iter().take(12).cloned().collect(),
        }
    }
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Task(task) => &task.id,
            Self::Onboarding { .. } => ONBOARDING_CHAT,
        }
    }
    pub(super) fn title(&self) -> &str {
        match self {
            Self::Task(task) => &task.title,
            Self::Onboarding { .. } => "Getting started with Kool.ad/e",
        }
    }
    pub(super) fn status(&self) -> BootstrapTaskStatusV1 {
        match self {
            Self::Task(task) => super::prompt::task_status(task.state),
            Self::Onboarding { .. } => BootstrapTaskStatusV1::Unknown,
        }
    }
    pub(super) fn facts(&self) -> Vec<BootstrapTaskFactV1> {
        match self {
            Self::Task(task) => super::prompt::facts(task),
            Self::Onboarding { tasks, .. } => {
                let mut facts = vec![BootstrapTaskFactV1 {
                    domain: BootstrapFactDomainV1::Onboarding,
                    status: BootstrapFactStatusV1::NotApplicable,
                    source: BootstrapFactSourceV1::Controller,
                    summary: format!(
                        "{} Up to 12 current checks are included, with checks needing attention first; open computer checks for the full list.",
                        self.guidance()
                    ),
                }];
                for task in tasks {
                    facts.extend(super::prompt::facts(task));
                }
                facts
            }
        }
    }
    pub(crate) fn guidance(&self) -> &str {
        let Self::Onboarding { step, .. } = self else {
            return "Review the selected setup task and its next step.";
        };
        match step {
            Step::Start => {
                "Kool.ad/e turns your idea into a plan. Get started introduces your AI assistant, checks this computer, and helps you choose a project. You approve changes before they run."
            }
            Step::Manager => {
                "Choose an AI tool in Coding Tools, follow its sign-in guidance, then refresh its checks. Choose it for Man.ager and setup help. Setup help can work before Docker; repository planning has separate readiness checks."
            }
            Step::Readiness => {
                "Open the checks to see what is ready and what needs attention. Each Fix action opens its own approval. You can continue to projects while a coding prerequisite is unavailable; continuing does not grant execution access."
            }
            Step::Projects => {
                "Open a recent workspace, connect an existing project folder, or start something new. Your planning history is kept separately from your application code."
            }
            Step::NewProject => {
                "Give your project a name and describe what you want to make. Choose a parent folder you can write to. Review shows the exact code and planning locations before anything is created."
            }
            Step::ReviewProject => {
                "Check both locations and your idea. Approve creates separate code and planning repositories and opens the planning interview. Existing files are never replaced. After an interruption, review this same request to resume."
            }
            Step::ExistingProject => {
                "Select the root folder of your project. Existing Git files and history are preserved. A folder without Git needs your explicit initialization approval. Choose a separate new or existing planning workspace before connecting."
            }
            Step::Advanced => {
                "Advanced options retain direct workspace opening and joining. These paths keep their existing checks and permissions."
            }
        }
    }
    pub(super) fn fingerprint(
        &self,
        host: crate::app::setup_attention::catalog::HostKind,
    ) -> String {
        match self {
            Self::Task(task) => super::task_fingerprint(task, host),
            Self::Onboarding {
                step,
                revision,
                tasks,
            } => {
                let evidence: Vec<_> = tasks
                    .iter()
                    .map(|task| super::task_fingerprint(task, host))
                    .collect();
                format!("onboarding:{step:?}:{revision}:{host:?}:{evidence:?}")
            }
        }
    }
    pub(super) fn reply(&self, reply: BootstrapConversationReplyV1) -> String {
        if let Some(explanation) = term_explanation(reply) {
            return explanation.into();
        }
        match self {
            Self::Task(task) => super::response_message(reply, task),
            Self::Onboarding { tasks, .. } => match reply {
                BootstrapConversationReplyV1::ExplainObservation => tasks.iter().find(|task| task.state != crate::persistence::setup_tasks::State::Verified)
                    .map(|task| super::response_message(reply, task)).unwrap_or_else(|| self.guidance().into()),
                BootstrapConversationReplyV1::RepeatNextStep => self.guidance().into(),
                _ => "Which part of this step would you like explained? Please do not include passwords, tokens or private project content.".into(),
            },
        }
    }
    pub(super) fn permits(&self, suggestion: &BootstrapIntentSuggestionV1) -> bool {
        match self {
            Self::Task(task) => super::suggest_remediation_intent(task, suggestion),
            Self::Onboarding { .. } => false,
        }
    }
}

pub(super) fn term_explanation(reply: BootstrapConversationReplyV1) -> Option<&'static str> {
    match reply {
        BootstrapConversationReplyV1::ExplainDocker => Some(
            "Docker provides isolated environments for running project tools and code. Kool.ad/e checks that the configured environment meets its safety requirements. You can choose a setup helper before Docker is ready. Open computer checks to review the exact requirement and any proposed repair.",
        ),
        BootstrapConversationReplyV1::ExplainGit => Some(
            "Git records changes so you can review a project's history. Kool.ad/e keeps plans in a separate Git repository from your app code. Connecting an existing Git project preserves its files and history; initializing a folder without Git requires your approval.",
        ),
        BootstrapConversationReplyV1::ExplainManager => Some(
            "Man.ager is your planning and coordination assistant. You choose which AI provider powers it. Setup help uses only the facts shown by Kool.ad/e and cannot run repairs. Repository planning and coding require their own readiness checks and approvals.",
        ),
        _ => None,
    }
}
