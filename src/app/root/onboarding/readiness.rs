//! Read-only presentation of current capability evidence. These summaries do
//! not authorize a turn or repair; execution revalidates its normal boundaries.
use super::*;
use crate::{
    execution::contract::HarnessRole,
    persistence::harness_settings::{self, HarnessSettings},
};

fn provider_label(id: &str) -> &str {
    match id {
        "codex" => "Codex",
        "claude" => "Claude Code",
        "opencode" => "OpenCode",
        "pi" => "Pi",
        _ => "Saved AI tool",
    }
}

pub(super) fn helper_ready(settings: &HarnessSettings) -> bool {
    let Some(id) = settings.bootstrap_provider.as_deref() else {
        return false;
    };
    let Some(entry) = settings.discovered.get(id) else {
        return false;
    };
    let now = harness_settings::current_unix_ms();
    entry.ready
        && !entry.configuration_required
        && crate::harness::execution_security::task_execution_block_reason(id).is_none()
        && entry.has_current_authenticated_capability(id, now)
        && entry.verified_default_model_at(id, now).is_some()
        && entry
            .capability_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.supports_context_only_help())
}

fn route_ready(settings: &HarnessSettings, category: &str, role: HarnessRole) -> bool {
    let route = settings.work_routes.get(category);
    let Some(id) = route
        .map(|route| route.harness.as_str())
        .or(settings.default_harness.as_deref())
    else {
        return false;
    };
    let Some(entry) = settings.discovered.get(id) else {
        return false;
    };
    let now = harness_settings::current_unix_ms();
    let model = route
        .and_then(|route| route.model.as_deref())
        .filter(|model| !model.is_empty())
        .or_else(|| entry.verified_default_model_at(id, now));
    entry.ready
        && !entry.configuration_required
        && crate::harness::execution_security::task_execution_block_reason(id).is_none()
        && entry.has_current_authenticated_capability(id, now)
        && entry.supports_role_at(role, now)
        && model.is_some_and(|model| entry.has_verified_model_at(id, model, now))
}

impl KooladeApp {
    pub(super) fn paint_manager_choice_status(&self, ui: &mut egui::Ui) {
        let settings = harness_settings::load().0;
        if let Some(route) = settings.work_routes.get(harness_settings::MANAGER) {
            ui.strong(format!(
                "Your Man.ager choice: {}",
                provider_label(&route.harness)
            ));
            ui.label(
                if route_ready(&settings, harness_settings::MANAGER, HarnessRole::Manager) {
                    "The selected tool has current verified Manager capability."
                } else {
                    "Choice saved. Repository planning still needs its separate readiness checks."
                },
            );
        } else {
            ui.label("Choose a tool deliberately. Kool.ad/e will not pick a provider for you.");
        }
        if let Some(id) = settings.bootstrap_provider.as_deref() {
            ui.label(format!(
                "Setup helper: {} · {}",
                provider_label(id),
                if helper_ready(&settings) {
                    "Ready"
                } else {
                    "Needs verification"
                }
            ));
        }
        ui.label("In Coding Tools, choose a provider, complete its own sign-in flow, and refresh its checks. Then choose it for Man.ager and setup help. Each provider's account and usage charges apply.");
        ui.collapsing("Can I use different tools?", |ui| {
            ui.label("Yes. Set the Manager route in Coding Tools and choose a separate setup helper there. Setup help can work before the repository environment is ready. Neither choice changes your other work routes.");
        });
    }

    pub(super) fn paint_readiness_summary(
        &self,
        ui: &mut egui::Ui,
        selection: &mut Option<String>,
    ) {
        use crate::persistence::setup_tasks::State;
        let settings = harness_settings::load().0;
        let checking = self.setup_probe.is_some();
        for (label, ready) in [
            ("Setup help", helper_ready(&settings)),
            (
                "Planning with Man.ager",
                route_ready(&settings, harness_settings::MANAGER, HarnessRole::Manager),
            ),
            (
                "Running coding tasks",
                route_ready(
                    &settings,
                    harness_settings::IMPLEMENTATION,
                    HarnessRole::Implementer,
                ),
            ),
        ] {
            ui.strong(format!(
                "{label}: {}",
                if checking {
                    "Checking…"
                } else if ready {
                    "Ready for the selected tool"
                } else {
                    "Needs setup or verification"
                }
            ));
        }
        ui.label("You can explore project setup while coding is unavailable. Each project and action is checked again.");
        let task = self
            .onboarding
            .active_setup_task_id
            .as_deref()
            .and_then(|id| {
                self.machine_setup_tasks.iter().find(|task| {
                    task.id == id && !matches!(task.state, State::Verified | State::Superseded)
                })
            })
            .or_else(|| {
                self.machine_setup_tasks
                    .iter()
                    .filter(|task| !matches!(task.state, State::Verified | State::Superseded))
                    .min_by_key(|task| {
                        (
                            !task.blocked_by.is_empty(),
                            !matches!(
                                task.state,
                                State::InProgress
                                    | State::NeedsApproval
                                    | State::AwaitingVerification
                            ),
                        )
                    })
            });
        if let Some(task) = task {
            ui.separator();
            let (title, benefit) = if task.id.starts_with("setup:git:") {
                (
                    "Save your work and history",
                    "Git keeps a reviewable history. A local project does not need a GitHub account. Planning history uses Kool.ad/e's own author identity; app-code identity and online access are separate settings.",
                )
            } else if task.id.starts_with("setup:cli:") {
                (
                    "Connect your chosen assistant",
                    "Your assistant needs a working account and a verified model. Sign in on the provider's own surface; Kool.ad/e does not collect passwords or keys.",
                )
            } else if task.id.starts_with("setup:docker:") || task.id.starts_with("setup:images:") {
                (
                    "Prepare a safe place to run changes",
                    "An isolated environment lets the assistant work without unrestricted access to your computer. Review the exact change before approving it.",
                )
            } else {
                (
                    "Check your computer’s setup",
                    "Review this check and its next action. Any change still needs its own permission.",
                )
            };
            ui.heading(title);
            ui.label(benefit);
            ui.label(match task.state {
                State::NeedsApproval => "Needs your permission",
                State::InProgress => "Working on it",
                State::AwaitingVerification => "Checking the result",
                State::Unsupported => "Cannot be set up automatically on this computer",
                State::Failed => "The last attempt failed; review the next action",
                State::BlockedByPrerequisite => "Complete its prerequisite first",
                State::Paused | State::Interrupted | State::Cancelled => {
                    "Paused; review before continuing"
                }
                _ => "Needs you to complete a step",
            });
            if ui
                .add(presentation::primary_button("Review this step"))
                .clicked()
            {
                *selection = Some(task.id.clone());
            }
        } else if !checking {
            ui.label(if self.machine_setup_tasks.is_empty() {
                "No setup task is recorded yet. Open the checks below to refresh readiness."
            } else {
                "No unresolved setup task is currently recorded."
            });
        }
    }
}
