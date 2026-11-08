use crate::execution::docker::{engine::HostPlatform, images::RuntimeProfile};

pub(super) fn profile_name(profile: RuntimeProfile) -> Option<(RuntimeProfile, &'static str)> {
    Some((
        profile,
        match profile {
            RuntimeProfile::General => "general",
            RuntimeProfile::Rust => "rust",
            RuntimeProfile::Node => "node",
            RuntimeProfile::Python => "python",
            RuntimeProfile::Go => "go",
            RuntimeProfile::BrowserQa | RuntimeProfile::DesktopQa => return None,
        },
    ))
}

pub(super) fn host_platform() -> HostPlatform {
    #[cfg(target_os = "macos")]
    return HostPlatform::MacOs;
    #[cfg(target_os = "windows")]
    return HostPlatform::Windows;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    HostPlatform::Linux
}
