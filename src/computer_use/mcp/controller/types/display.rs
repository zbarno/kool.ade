mod recovered;
mod streams;

use sha2::Digest;
use std::sync::Arc;

use super::{ActiveSession, ComputerUseMcpControllerV1};
use crate::computer_use::mcp::controller::unix_time_ms;
use crate::computer_use::mcp::{
    ComputerUseTaskSessionStatusV1 as Status, ComputerUseTaskSessionViewV1,
};

impl ComputerUseMcpControllerV1 {
    pub fn task_session_views(
        self: &Arc<Self>,
        task_id: &str,
        user_id: &str,
    ) -> Vec<ComputerUseTaskSessionViewV1> {
        let Ok(cache) = self.view_cache.lock() else {
            return Vec::new();
        };
        let cached = cache
            .iter()
            .filter(|(_, view)| view.task_id == task_id && view.user_id == user_id)
            .map(|(id, view)| (id.clone(), view.clone()))
            .collect::<Vec<_>>();
        drop(cache);
        let mut views = cached
            .into_iter()
            .map(|(id, mut view)| {
                self.ensure_display_stream(&id);
                let (frame_revision, frame_width, frame_height, frame_png, frame_error) = self
                    .display_streams
                    .lock()
                    .ok()
                    .and_then(|streams| streams.get(&id).cloned())
                    .and_then(|snapshot| {
                        snapshot.lock().ok().map(|s| {
                            (
                                s.frame_revision,
                                s.frame_width,
                                s.frame_height,
                                s.frame_png.clone(),
                                s.frame_error,
                            )
                        })
                    })
                    .unwrap_or_default();
                if view.status == Status::Queued {
                    view.queue_position = self
                        .runtime
                        .as_ref()
                        .and_then(|runtime| runtime.scheduler.queue_position(&id));
                }
                view.remaining_ms = view.expires_at_unix_ms.saturating_sub(unix_time_ms());
                view.frame_revision = frame_revision;
                view.frame_width = frame_width;
                view.frame_height = frame_height;
                view.frame_png = if view.private_input { None } else { frame_png };
                view.frame_error = frame_error;
                if let Ok(pending) = self.pending_controls.lock()
                    && let Some(error) = pending.get(&id)
                {
                    view.control_transition_pending = error.is_none();
                    view.control_error = error.clone();
                }
                view
            })
            .collect::<Vec<_>>();
        let known = views
            .iter()
            .map(|view| view.session_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        views.extend(recovered::views(self, task_id, user_id, &known));
        views.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        views
    }

    pub(in crate::computer_use::mcp::controller) fn cache_active_view(
        &self,
        active: &ActiveSession,
    ) {
        let session = &active.session;
        let view = ComputerUseTaskSessionViewV1 {
            session_id: session.identity.session_id.clone(),
            task_id: session.identity.task_id.clone(),
            user_id: session.identity.user_id.clone(),
            agent_role: active.role_id.clone(),
            session_class: session.session_class,
            allowed_apps: session.allowed_apps.iter().cloned().collect(),
            guest_apps: self.guest_app_views(active),
            guest_app_catalog_status: self
                .runtime
                .as_ref()
                .map_or("trusted_runtime_unavailable", |runtime| {
                    runtime.application_catalog_status
                })
                .to_owned(),
            app_management_available: active
                .channel
                .as_ref()
                .is_some_and(|channel| channel.capabilities().app_management)
                && self
                    .runtime
                    .as_ref()
                    .is_some_and(|runtime| runtime.application_catalog.is_some()),
            resource_budget: session.policy.resource_budget.clone(),
            actions_remaining: session
                .policy
                .resource_budget
                .action_limit
                .saturating_sub(active.next_action_sequence.saturating_sub(1) as u32),
            lifecycle: session.lifecycle,
            status: if active.uncertain {
                Status::Uncertain
            } else if matches!(
                session.lifecycle,
                crate::computer_use::ComputerLifecycleV1::Interrupted
                    | crate::computer_use::ComputerLifecycleV1::Recovering
            ) {
                Status::Interrupted
            } else {
                Status::Active
            },
            queue_position: None,
            control_owner: active.control_owner,
            control_generation: active.control_generation,
            active_app: active.active_app.clone(),
            remaining_ms: session
                .lease
                .expires_at_unix_ms
                .saturating_sub(unix_time_ms()),
            expires_at_unix_ms: session.lease.expires_at_unix_ms,
            frame_revision: 0,
            frame_width: 0,
            frame_height: 0,
            frame_png: None,
            frame_error: false,
            private_input: active.private_input,
            control_transition_pending: false,
            control_error: None,
            control_notice: active.inspection_result.clone(),
            action_log: active.action_log.iter().cloned().collect(),
            pending_action_intent: active.pending_action_intent.as_ref().map(|pending| {
                crate::computer_use::mcp::ComputerUseActionIntentViewV1 {
                    intent: pending.intent.clone(),
                    final_action: pending.final_action.clone(),
                    state: pending.state,
                }
            }),
        };
        if let Ok(mut views) = self.view_cache.lock() {
            views.insert(session.identity.session_id.clone(), view);
        }
    }

    fn guest_app_views(
        &self,
        active: &ActiveSession,
    ) -> Vec<crate::computer_use::mcp::ComputerUseGuestAppViewV1> {
        let Some(catalog) = self
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.application_catalog.as_ref())
        else {
            return Vec::new();
        };
        let installed = active
            .channel
            .as_ref()
            .map(|channel| channel.capabilities().app_registry.as_slice())
            .unwrap_or_default();
        catalog
            .catalog()
            .apps
            .iter()
            .filter_map(|manifest| {
                let bytes = serde_json::to_vec(manifest).ok()?;
                Some(crate::computer_use::mcp::ComputerUseGuestAppViewV1 {
                    app_id: manifest.app_id.clone(),
                    display_name: manifest.display_name.clone(),
                    publisher: manifest.package.publisher.clone(),
                    source_id: manifest.package.source_id.clone(),
                    provenance: manifest.package.provenance,
                    version: manifest.package.version.clone(),
                    package_id: manifest.package.package_id.clone(),
                    package_sha256: manifest.package.package_sha256.clone(),
                    manifest_sha256: format!("sha256:{:x}", sha2::Sha256::digest(bytes)),
                    required_capabilities: manifest.required_capabilities.iter().copied().collect(),
                    installed: installed.iter().any(|app| app == &manifest.app_id),
                    session_allowed: active.session.allowed_apps.contains(&manifest.app_id),
                })
            })
            .collect()
    }

    pub(in crate::computer_use::mcp::controller) fn cache_queued_view(
        &self,
        caller: &super::super::super::ComputerUseCallerV1,
        budget: crate::computer_use::ComputerResourceBudgetV1,
        expires_at_unix_ms: u64,
    ) {
        let view = ComputerUseTaskSessionViewV1 {
            session_id: caller.identity.session_id.clone(),
            task_id: caller.identity.task_id.clone(),
            user_id: caller.identity.user_id.clone(),
            agent_role: caller.role_id.clone(),
            session_class: crate::computer_use::ComputerSessionClassV1::Disposable,
            allowed_apps: Vec::new(),
            guest_apps: Vec::new(),
            guest_app_catalog_status: "signed_app_catalog_unavailable".into(),
            app_management_available: false,
            actions_remaining: budget.action_limit,
            resource_budget: budget,
            lifecycle: crate::computer_use::ComputerLifecycleV1::Preparing,
            status: Status::Queued,
            queue_position: None,
            control_owner: crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent,
            control_generation: 1,
            active_app: None,
            remaining_ms: expires_at_unix_ms.saturating_sub(unix_time_ms()),
            expires_at_unix_ms,
            frame_revision: 0,
            frame_width: 0,
            frame_height: 0,
            frame_png: None,
            frame_error: false,
            private_input: false,
            control_transition_pending: false,
            control_error: None,
            control_notice: None,
            action_log: Vec::new(),
            pending_action_intent: None,
        };
        if let Ok(mut views) = self.view_cache.lock() {
            views.insert(caller.identity.session_id.clone(), view);
        }
    }

    pub(in crate::computer_use::mcp::controller) fn set_start_status(
        &self,
        session_id: &str,
        status: Status,
    ) {
        if let Ok(mut views) = self.view_cache.lock()
            && let Some(view) = views.get_mut(session_id)
        {
            view.status = status;
            view.queue_position = None;
        }
    }

    pub(in crate::computer_use::mcp::controller) fn remove_cached_view(&self, session_id: &str) {
        if let Ok(mut views) = self.view_cache.lock() {
            views.remove(session_id);
        }
        if let Ok(mut streams) = self.display_streams.lock() {
            streams.remove(session_id);
        }
    }

    pub(in crate::computer_use::mcp::controller) fn control_change_pending(
        &self,
        session_id: &str,
    ) -> bool {
        self.pending_controls
            .lock()
            .is_ok_and(|pending| pending.get(session_id).is_some_and(Option::is_none))
    }
}
