//! Task-detail routing notice: when coding work cannot be routed to a
//! healthy CLI, the operator deserves an actionable nudge in the task
//! views instead of discovering the block only after a task parks.
//! Kool.ad/e never infers a default silently; the notice names the gap
//! and, when exactly one provider is actionable, narrows the choice to
//! a single click in the Coding tools dialog.

use super::routing::route_selection;
use crate::persistence::harness_settings::{HarnessSettings, current_unix_ms};

/// A human-facing routing gap plus, when exactly one provider is
/// actionable, its identifier so the choice collapses to one click.
pub(crate) struct CliRoutingNotice {
    pub message: String,
    pub ready_provider: Option<String>,
}

static EMPTY_ROUTES: std::sync::OnceLock<
    std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
> = std::sync::OnceLock::new();

fn empty_project_routes()
-> &'static std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute> {
    EMPTY_ROUTES.get_or_init(std::collections::BTreeMap::new)
}

/// Derive the routing notice for the global (no project/task overrides)
/// selection, honoring the legacy env override exactly like the router
/// does (see [`super::routing::legacy_env_override`]). Returns `None`
/// when coding work can route to a healthy CLI.
pub(crate) fn cli_routing_notice(settings: &HarnessSettings) -> Option<CliRoutingNotice> {
    let legacy = (std::env::var(crate::harness::CODEX_HARNESS_ENV).as_deref() == Ok("codex"))
        .then_some("codex");
    cli_routing_notice_with_legacy(settings, legacy)
}

/// Inner derivation with the legacy-env override injected, so unit tests
/// are hermetic (no process-wide `set_var`, immune to a host that exports
/// the legacy knob).
pub(crate) fn cli_routing_notice_with_legacy(
    settings: &HarnessSettings,
    legacy: Option<&str>,
) -> Option<CliRoutingNotice> {
    let now = current_unix_ms();
    let ready: Vec<String> = settings
        .discovered
        .iter()
        .filter(|(id, entry)| entry.is_fresh_at(now) && entry.is_currently_actionable(id, now))
        .map(|(id, _)| id.clone())
        .collect();
    let sole = (ready.len() == 1).then(|| ready.into_iter().next().expect("length is one"));
    let (selected, _) = route_selection(settings, empty_project_routes(), None, legacy, None);
    match selected {
        None => Some(CliRoutingNotice {
            message: "No coding CLI is selected yet. Choose one to run coding work.".into(),
            ready_provider: sole,
        }),
        Some(id) => {
            let entry = settings.discovered.get(id);
            if entry.is_some_and(|entry| {
                entry.is_fresh_at(now) && entry.is_currently_actionable(id, now)
            }) {
                return None;
            }
            let blocked = crate::harness::execution_security::task_execution_block_reason(id);
            let message = match entry {
                None => format!(
                    "The saved CLI '{id}' has not been discovered. Open Coding tools and rediscover it."
                ),
                Some(entry) if blocked.is_some() && entry.is_fresh_at(now) => {
                    // Build-level refusal: re-checking cannot fix it, so
                    // say so instead of prescribing a futile re-probe.
                    let reason = blocked.expect("matched Some");
                    let reason_head = reason.split('.').next().unwrap_or(reason);
                    format!(
                        "CLI '{id}' is selected, but this build will not route task execution to it: {reason_head} Choose a different CLI."
                    )
                }
                Some(_) => format!(
                    "CLI '{id}' is selected but is not reporting current capability. Re-check it in Coding tools."
                ),
            };
            Some(CliRoutingNotice {
                message,
                ready_provider: sole.filter(|candidate| candidate != id),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{cli_routing_notice_with_legacy as notice, *};
    use crate::execution::contract::{HarnessCapabilitySnapshotV1, ReadinessState};
    use crate::persistence::harness_settings::DetectedHarness;

    fn snapshot(provider: &str, auth: ReadinessState) -> HarnessCapabilitySnapshotV1 {
        HarnessCapabilitySnapshotV1 {
            schema_version: 1,
            provider_id: provider.into(),
            probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            probe_cancellation_generation: 1,
            probe_sequence: 1,
            cli_binary: ReadinessState::Ready,
            cli_version: Some("1.0.0".into()),
            authentication: auth,
            model_catalog: ReadinessState::Ready,
            scriptable_invocation: ReadinessState::Ready,
            docker_mcp_bridge: ReadinessState::Ready,
            execution_target: ReadinessState::Ready,
            failure: None,
            modes: crate::harness::ExecutionMode::ALL
                .into_iter()
                .map(|mode| crate::execution::contract::ModeReadinessV1 {
                    mode,
                    available: ReadinessState::Ready,
                    failure: None,
                })
                .collect(),
            roles: crate::execution::contract::HarnessRole::ALL
                .into_iter()
                .map(|role| crate::execution::contract::RoleReadinessV1 {
                    role,
                    available: ReadinessState::Ready,
                    failure: None,
                })
                .collect(),
        }
    }

    fn detected(provider: &str, auth: ReadinessState, checked_at_unix_ms: u64) -> DetectedHarness {
        DetectedHarness {
            status: format!("{provider} ready"),
            version: Some("1.0.0".into()),
            executable: Some(format!("/synthetic/bin/{provider}")),
            diagnostic: None,
            ready: auth == ReadinessState::Ready,
            models: vec!["synthetic-default".into()],
            default_model: Some("synthetic-default".into()),
            model_catalog_fresh: true,
            configuration_required: false,
            implementation_available: true,
            checked_at_unix_ms,
            capability_snapshot: Some(snapshot(provider, auth)),
        }
    }

    fn settings(
        default: Option<&str>,
        discovered: &[(&str, ReadinessState)],
        age_ms: u64,
    ) -> HarnessSettings {
        let now = current_unix_ms();
        let mut built = HarnessSettings {
            default_harness: default.map(str::to_owned),
            ..HarnessSettings::default()
        };
        for (provider, auth) in discovered {
            built.discovered.insert(
                (*provider).to_string(),
                detected(provider, *auth, now.saturating_sub(age_ms)),
            );
        }
        built
    }

    fn notice_of(settings: &HarnessSettings) -> Option<CliRoutingNotice> {
        notice(settings, None)
    }

    const READY: ReadinessState = ReadinessState::Ready;
    #[test]
    fn no_default_names_the_gap_and_suggests_a_sole_ready_provider() {
        let notice = notice_of(&settings(None, &[("pi", READY)], 0)).unwrap();
        assert!(notice.message.contains("No coding CLI is selected"));
        assert_eq!(notice.ready_provider.as_deref(), Some("pi"));
    }

    #[test]
    fn no_unique_ready_provider_offers_no_suggestion() {
        // Several ready providers (never infer a winner):
        let notice =
            cli_routing_notice(&settings(None, &[("pi", READY), ("copilot", READY)], 0)).unwrap();
        assert_eq!(notice.ready_provider, None);
        // A freshly probed but execution-blocked provider offers nothing:
        let notice = notice_of(&settings(None, &[("claude", READY)], 0)).unwrap();
        assert_eq!(notice.ready_provider, None);
    }

    #[test]
    fn healthy_selected_cli_raises_no_notice() {
        assert!(notice_of(&settings(Some("pi"), &[("pi", READY)], 0)).is_none());
    }

    #[test]
    fn unactionable_selected_cli_is_called_out_with_a_different_ready_candidate() {
        // Claude is blocked from task execution in this build, so a
        // freshly probed Claude entry is still not routable: the notice
        // must call it out and suggest the one genuinely ready provider.
        let settings = settings(Some("claude"), &[("claude", READY), ("pi", READY)], 0);
        let notice = notice_of(&settings).unwrap();
        assert!(
            notice
                .message
                .contains("will not route task execution to it")
        );
        assert_eq!(notice.ready_provider.as_deref(), Some("pi"));
    }

    #[test]
    fn selected_but_undiscovered_cli_points_at_rediscovery() {
        let notice = notice_of(&settings(Some("claude"), &[], 0)).unwrap();
        assert!(notice.message.contains("not been discovered"));
        assert_eq!(notice.ready_provider, None);
    }
}
