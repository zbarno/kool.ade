use super::{super::QaPlatform, QaDriverActionV1};

pub(super) fn action_platform(action: &QaDriverActionV1) -> Option<QaPlatform> {
    match action {
        QaDriverActionV1::LaunchValidatedTarget
        | QaDriverActionV1::WaitForWindow
        | QaDriverActionV1::ListWindows
        | QaDriverActionV1::FocusWindow { .. }
        | QaDriverActionV1::ResizeWindow { .. }
        | QaDriverActionV1::Click { .. }
        | QaDriverActionV1::DoubleClick { .. }
        | QaDriverActionV1::Drag { .. }
        | QaDriverActionV1::TypeText { .. }
        | QaDriverActionV1::KeyChord { .. }
        | QaDriverActionV1::Scroll { .. }
        | QaDriverActionV1::Screenshot
        | QaDriverActionV1::ReadAppLogs
        | QaDriverActionV1::CloseWindow { .. } => Some(QaPlatform::LinuxDesktop),
        QaDriverActionV1::WaitForVisible { .. }
        | QaDriverActionV1::NavigateAllowedUrl { .. }
        | QaDriverActionV1::FindByRole { .. }
        | QaDriverActionV1::FindByTestId { .. }
        | QaDriverActionV1::Fill { .. }
        | QaDriverActionV1::Press { .. }
        | QaDriverActionV1::Select { .. }
        | QaDriverActionV1::CollectConsole
        | QaDriverActionV1::CollectNetworkFailures
        | QaDriverActionV1::BrowserOpen { .. }
        | QaDriverActionV1::BrowserClick { .. }
        | QaDriverActionV1::BrowserFill { .. }
        | QaDriverActionV1::BrowserPress { .. }
        | QaDriverActionV1::BrowserSelect { .. }
        | QaDriverActionV1::BrowserWaitForVisible { .. }
        | QaDriverActionV1::BrowserAssertText { .. }
        | QaDriverActionV1::BrowserAssertUrl { .. }
        | QaDriverActionV1::BrowserSetViewport { .. }
        | QaDriverActionV1::BrowserScreenshot
        | QaDriverActionV1::BrowserScroll { .. } => Some(QaPlatform::Web),
    }
}
