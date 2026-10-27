use super::super::ClaudeInstallProgress;

pub(super) fn advance_progress(
    current: &mut Option<ClaudeInstallProgress>,
    next: ClaudeInstallProgress,
) {
    let order = |phase| match phase {
        ClaudeInstallProgress::Download => 0,
        ClaudeInstallProgress::TrustVerification => 1,
        ClaudeInstallProgress::CurrentUserInstall => 2,
        ClaudeInstallProgress::FreshProviderProbe => 3,
    };
    if current.is_none_or(|current| order(current) <= order(next)) {
        *current = Some(next);
    }
}
