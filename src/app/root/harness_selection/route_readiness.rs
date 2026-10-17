pub(super) fn has_current_authenticated_snapshot(
    harness_id: &str,
    detected: &crate::persistence::harness_settings::DetectedHarness,
    now_unix_ms: u64,
) -> bool {
    use crate::execution::contract::ReadinessState;

    detected.is_fresh_at(now_unix_ms)
        && detected.ready
        && detected
            .executable
            .as_deref()
            .is_some_and(|path| !path.trim().is_empty())
        && detected
            .capability_snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot.validate().is_ok()
                    && snapshot.provider_id == harness_id
                    && snapshot.cli_binary == ReadinessState::Ready
                    && snapshot.authentication == ReadinessState::Ready
                    && snapshot.cli_version == detected.version
            })
}
