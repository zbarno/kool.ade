pub(super) fn desktop_steps() -> &'static [&'static str] {
    &[
        "If Docker Desktop is missing, use the official Mac guide linked below and choose the installer for this Mac's processor.",
        "Open Docker Desktop from Applications and complete its first-run steps.",
        "Review the Docker Desktop license and ordinary macOS security prompts. Do not bypass Gatekeeper or disable system protections.",
        "Wait until Docker Desktop's Linux environment is running, then choose Check again.",
    ]
}

pub(super) fn disk_steps() -> &'static [&'static str] {
    &[
        "Open Docker Desktop Settings from its menu and review Resources.",
        "Review the disk image location and size using the official Docker help links below.",
        "Return here and choose Check again so Kool.ad/e can remeasure VM capacity. Mac host free space alone is not proof of VM capacity.",
    ]
}

pub(super) fn limits_steps() -> &'static [&'static str] {
    &[
        "Open Docker Desktop Settings and review the CPU and memory limits under Resources.",
        "Do not increase limits beyond what this Mac can safely provide.",
        "Return here and choose Check again; coding tasks stay paused until the app verifies the VM limits.",
    ]
}

pub(super) fn endpoint_steps() -> &'static [&'static str] {
    &[
        "Close Docker Desktop if Kool.ad/e could not verify its publisher or local connection.",
        "Use the official Docker Desktop for Mac guide below to install a fresh copy, or choose its local Linux context if the app was verified.",
        "Do not bypass macOS security warnings or use TCP, SSH, or a remote Docker context.",
        "Return here and choose Check again after the local app and socket have been verified.",
    ]
}

pub(super) fn setup_links() -> Vec<(&'static str, &'static str)> {
    vec![(
        "Docker Desktop for Mac guide",
        "https://docs.docker.com/desktop/setup/install/mac-install/",
    )]
}

pub(super) fn resource_links() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "Docker Desktop resources and disk image settings",
            "https://docs.docker.com/desktop/settings-and-maintenance/settings/",
        ),
        (
            "Docker Desktop disk image details",
            "https://docs.docker.com/desktop/troubleshoot-and-support/faqs/macfaqs/",
        ),
    ]
}
