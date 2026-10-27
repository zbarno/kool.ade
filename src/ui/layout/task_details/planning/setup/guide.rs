use crate::{app::setup_attention::catalog::HostKind, persistence::setup_tasks::LocalTask};

mod macos;

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
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("What to do"));
    for (index, step) in steps(task, host).iter().enumerate() {
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
    match task.id.as_str() {
        "setup:docker:install" => match host {
            HostKind::Linux => &[
                "Open Docker's installation guide for your Linux distribution.",
                "Choose Docker Engine and follow the rootless setup steps for your account.",
                "Return here and choose Check again when the service is ready.",
            ],
            HostKind::MacOs => &[
                "Install Docker Desktop using Docker's Mac guide.",
                "Open Docker Desktop and wait for its Linux environment to start.",
                "Return here and choose Check again.",
            ],
            HostKind::Windows => &[
                "Install Docker Desktop using Docker's Windows guide.",
                "Use the Linux container backend. WSL 2 setup may ask for one-time Windows approval.",
                "Start Docker Desktop, then choose Check again.",
            ],
            HostKind::Unsupported => &[
                "Kool.ad/e has not certified coding-task execution on this operating system.",
                "Choose a supported operating system before enabling coding tasks.",
            ],
        },
        "setup:docker:start" => match host {
            HostKind::MacOs => &[
                "Open Docker Desktop from Applications.",
                "Wait until Docker Desktop says it is running, then choose Check again.",
            ],
            HostKind::Windows => &[
                "Open Docker Desktop from the Start menu.",
                "Wait until Docker Desktop says it is running, then choose Check again.",
            ],
            HostKind::Linux => &[
                "Open your system's service manager and start the Docker service for your user.",
                "Return here and choose Check again after the service is ready.",
            ],
            HostKind::Unsupported => {
                &["Coding-task execution is not supported on this operating system."]
            }
        },
        "setup:docker:rootless" => &[
            "Set Docker up to run under your Linux account.",
            "Do not grant Docker administrator-level access just to pass this check.",
            "Choose Check again after the service is ready.",
        ],
        "setup:docker:permission" => &[
            "Confirm that the selected service belongs to this machine and your account.",
            "Avoid granting administrator-equivalent access just to dismiss this check.",
            "Choose Check again after access is corrected.",
        ],
        "setup:docker:desktop" => match host {
            HostKind::MacOs => macos::desktop_steps(),
            HostKind::Windows => &[
                "Open Docker Desktop and turn on its Linux container backend.",
                "If prompted, allow the one-time WSL 2 Windows feature setup.",
                "Wait for Docker Desktop to finish starting, then choose Check again.",
            ],
            _ => &[
                "Open Docker Desktop and start its Linux environment.",
                "Wait until the engine is running, then choose Check again.",
            ],
        },
        "setup:docker:disk" if host == HostKind::MacOs => macos::disk_steps(),
        "setup:docker:limits" if host == HostKind::MacOs => macos::limits_steps(),
        "setup:docker:endpoint" if host == HostKind::MacOs => macos::endpoint_steps(),
        "setup:docker:desktop-transport" => &[
            "This setup check does not enable coding-task execution on this platform.",
            "Use the vendor guide for setup information; wait for a release-certified platform update before running coding tasks.",
        ],
        "setup:cli:install" => &[
            "Choose a supported coding agent in Coding Tools when you are ready.",
            "You can keep planning without installing a coding agent.",
        ],
        "setup:cli:scan" => &[
            "Choose Check again to repeat discovery.",
            "If the check keeps failing, open Technical details and share only the redacted status with support.",
        ],
        _ => &[
            "Review the explanation and recommended next step.",
            "Choose Check again after making a change, or open Coding Tools for account settings.",
        ],
    }
}

pub(super) fn help_links(task: &LocalTask, host: HostKind) -> Vec<(&'static str, &'static str)> {
    match task.id.as_str() {
        "setup:docker:rootless" => vec![(
            "Docker rootless Engine guide",
            "https://docs.docker.com/engine/security/rootless/",
        )],
        "setup:docker:install" | "setup:docker:start" | "setup:docker:desktop"
            if host == HostKind::MacOs =>
        {
            macos::setup_links()
        }
        "setup:docker:disk" | "setup:docker:limits" if host == HostKind::MacOs => {
            macos::resource_links()
        }
        "setup:docker:endpoint" if host == HostKind::MacOs => macos::setup_links(),
        "setup:docker:disk" | "setup:docker:limits" if host == HostKind::MacOs => vec![
            (
                "Docker Desktop resources and disk image settings",
                "https://docs.docker.com/desktop/settings-and-maintenance/settings/",
            ),
            (
                "Docker Desktop disk image details",
                "https://docs.docker.com/desktop/troubleshoot-and-support/faqs/macfaqs/",
            ),
        ],
        "setup:docker:install" | "setup:docker:start" | "setup:docker:desktop"
            if host == HostKind::Windows =>
        {
            vec![
                (
                    "Docker Desktop for Windows guide",
                    "https://docs.docker.com/desktop/setup/install/windows-install/",
                ),
                (
                    "Docker Desktop with WSL 2",
                    "https://docs.docker.com/desktop/features/wsl/",
                ),
                (
                    "Install WSL",
                    "https://learn.microsoft.com/windows/wsl/install",
                ),
            ]
        }
        "setup:docker:install" | "setup:docker:start" | "setup:docker:desktop"
            if host == HostKind::Linux =>
        {
            vec![(
                "Docker Engine installation guide",
                "https://docs.docker.com/engine/install/",
            )]
        }
        "setup:docker:desktop-transport" if host == HostKind::Windows => vec![
            (
                "Docker Desktop with WSL 2",
                "https://docs.docker.com/desktop/features/wsl/",
            ),
            (
                "Install WSL",
                "https://learn.microsoft.com/windows/wsl/install",
            ),
        ],
        "setup:docker:desktop-transport" if host == HostKind::MacOs => vec![(
            "Docker Desktop for Mac guide",
            "https://docs.docker.com/desktop/setup/install/mac-install/",
        )],
        _ => Vec::new(),
    }
}
