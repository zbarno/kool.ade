use crate::{app::setup_attention::catalog::HostKind, persistence::setup_tasks::LocalTask};

mod instructions;
mod providers;

pub(super) fn plain_title(task: &LocalTask) -> String {
    match task.id.as_str() {
        "setup:docker:install" => "Set up a safe coding environment".into(),
        "setup:docker:start" | "setup:docker:desktop" => "Start the safe coding environment".into(),
        "setup:docker:rootless" | "setup:docker:permission" => {
            "Allow coding tasks to run safely".into()
        }
        "setup:docker:desktop-transport" => "Check support on this computer".into(),
        "setup:docker:endpoint" => "Choose a trusted local service".into(),
        "setup:docker:capabilities" => "Check coding task requirements".into(),
        "setup:cli:scan" => "Check for coding assistants".into(),
        id if let Some((profile, _, _)) =
            crate::app::bootstrap_repair_policy::parse_locked_image_task_id(id) =>
        {
            format!("Prepare the {profile} runtime image")
        }
        _ => task.title.clone(),
    }
}

pub(super) fn summary(task: &LocalTask) -> &'static str {
    match task.id.as_str() {
        "setup:docker:install" => {
            "Kool.ad/e needs a protected local environment before it can run coding tasks."
        }
        "setup:docker:start" => {
            "The safe local environment is installed, but it is not ready to use yet."
        }
        "setup:docker:rootless" => {
            "Use Docker through your own account. Coding tasks then run without giving it administrator-level access."
        }
        "setup:docker:permission" => "Kool.ad/e cannot safely connect using the current account.",
        "setup:docker:desktop" => {
            "The app's local Linux coding environment is not ready. Coding tasks stay paused until it is available."
        }
        "setup:docker:capabilities" => {
            "The local environment did not pass the safety or resource checks."
        }
        "setup:docker:desktop-transport" => {
            "This platform can be detected, but coding-task execution through Docker Desktop is not release-certified here. This task stays unavailable."
        }
        "setup:docker:endpoint" => "The selected service cannot be confirmed as local and trusted.",
        "setup:cli:install" => {
            "You can explore and plan with Kool.ad/e before choosing an AI coding tool."
        }
        "setup:cli:scan" => {
            "A coding-tool check did not finish reliably. This result is unknown, so Kool.ad/e will not treat it as ready."
        }
        _ => "Kool.ad/e found a setup step that needs attention on this machine.",
    }
}

pub(super) fn paint(ui: &mut egui::Ui, task: &LocalTask, host: HostKind) {
    ui.label(crate::ui::theme::section_heading("Setup conversation"));
    crate::ui::theme::card_frame().show(ui, |ui| {
        ui.label("I checked this machine and found a setup step that needs your attention.");
        ui.label(summary(task));
        ui.label("I’ll show the safest next step here. You stay in control of any repair.");
    });
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("Setup guide"));
    ui.label(summary(task));
    if let Some((provider, stage)) = providers::provider_task(task) {
        providers::paint_progress(ui, task, provider);
        ui.label(provider.install_note(host));
        ui.label(crate::ui::theme::helper_text(
            provider.install_channel_note(host),
        ));
        ui.label(provider_task_guidance(provider, stage));
        ui.label("Provider accounts, plans, and model usage terms vary. Review the vendor's account and pricing terms before signing in; Kool.ad/e does not enroll you or make purchases.");
        ui.label("Complete sign-in and model choices on the provider's own surface. Do not paste passwords, tokens, API keys, or browser data into Kool.ad/e.");
    }
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("What to do"));
    let rendered_steps = rendered_steps(task, host);
    for (index, step) in rendered_steps.iter().enumerate() {
        ui.horizontal_top(|ui| {
            crate::ui::theme::badge(
                ui,
                &format!("{}", index + 1),
                crate::ui::theme::PANEL_ALT,
                crate::ui::theme::TEXT,
            );
            ui.label(*step);
        });
    }
    if !task.explanation.is_empty() && task.explanation != summary(task) {
        ui.add_space(crate::ui::theme::spacing::M);
        ui.collapsing("Why this matters", |ui| ui.label(&task.explanation));
    }
    let links = help_links(task, host);
    if !links.is_empty() {
        ui.add_space(crate::ui::theme::spacing::S);
        ui.label(crate::ui::theme::section_heading("Official help"));
        for (label, url) in links {
            ui.hyperlink_to(label, url);
        }
    }
}

pub(super) fn status_message(state: crate::persistence::setup_tasks::State) -> &'static str {
    use crate::persistence::setup_tasks::State;
    match state {
        State::Detected => "We found something that needs attention.",
        State::ReadyToFix => "This setup step is ready for you to review.",
        State::BlockedByPrerequisite => "Another setup step needs to pass first.",
        State::NeedsApproval => "Waiting for you to approve the proposed repair.",
        State::InProgress => "The approved repair is running.",
        State::AwaitingVerification => "The repair finished. Checking the result now.",
        State::NeedsVerification => "The current result is not verified yet.",
        State::Verified => "This setup step passed its latest check.",
        State::Failed => "The last repair did not finish. Review it before trying again.",
        State::Unsupported => "Unavailable on this operating system. This is not a ready state.",
        State::Cancelled => "The repair was cancelled. You can review it again when ready.",
        State::Paused => "The repair is paused. Review the next step before continuing.",
        State::Interrupted => {
            "The app stopped before it could confirm this repair. Check again before deciding what to do."
        }
        State::Superseded => "A more specific setup check replaced this older task.",
    }
}

pub(super) fn humanize_prerequisite(id: &str) -> String {
    match id {
        "setup:docker:engine" => "Allow coding tasks to run safely".into(),
        "setup:docker:rootless" => "Configure secure Docker for this user".into(),
        "setup:docker:capabilities" => "Check Docker security and resources".into(),
        "setup:cli:install" => "Choose a coding agent".into(),
        "setup:cli:scan" => "Check coding agent discovery".into(),
        _ if id.starts_with("setup:cli:") => "Finish coding agent setup".into(),
        _ if let Some((profile, _, _)) =
            crate::app::bootstrap_repair_policy::parse_locked_image_task_id(id) =>
        {
            format!("Prepare the pinned {profile} runtime image")
        }
        _ => "Complete another machine setup step".into(),
    }
}

pub(super) fn steps(task: &LocalTask, host: HostKind) -> &'static [&'static str] {
    instructions::steps(task, host)
}

pub(super) fn help_links(task: &LocalTask, host: HostKind) -> Vec<(&'static str, &'static str)> {
    instructions::help_links(task, host)
}

pub(super) fn provider_task_guidance(
    provider: crate::app::setup_attention::catalog::ProviderIdV1,
    stage: &str,
) -> &'static str {
    providers::provider_task_guidance(provider, stage)
}

#[cfg(test)]
pub(super) fn provider_progress(
    task: &LocalTask,
) -> Option<(&'static str, &'static str, &'static str)> {
    providers::provider_progress(task)
        .map(|(provider, stage, state)| (provider.display_name(), stage, state))
}

pub(super) fn rendered_steps(task: &LocalTask, host: HostKind) -> Vec<&'static str> {
    providers::provider_task(task)
        .map(|(provider, stage)| providers::provider_steps(provider, stage))
        .unwrap_or_else(|| steps(task, host).to_vec())
}
