use super::{HostKind, providers};
use crate::persistence::setup_tasks::LocalTask;

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
    if let Some((provider, stage)) = providers::provider_task(task) {
        return providers::provider_links(provider, stage);
    }
    match task.id.as_str() {
        "setup:docker:rootless" => vec![(
            "Docker rootless Engine guide",
            "https://docs.docker.com/engine/security/rootless/",
        )],
        "setup:docker:install" | "setup:docker:start" | "setup:docker:desktop"
            if host == HostKind::MacOs =>
        {
            vec![(
                "Docker Desktop for Mac guide",
                "https://docs.docker.com/desktop/setup/install/mac-install/",
            )]
        }
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
