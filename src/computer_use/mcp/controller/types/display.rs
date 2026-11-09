use std::{
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

use super::{ActiveSession, ComputerUseMcpControllerV1, DisplaySnapshot};
use crate::computer_use::mcp::ComputerUseTaskSessionViewV1;
use crate::computer_use::mcp::controller::unix_time_ms;

impl ComputerUseMcpControllerV1 {
    pub fn task_session_views(
        self: &Arc<Self>,
        task_id: &str,
        user_id: &str,
    ) -> Vec<ComputerUseTaskSessionViewV1> {
        let Ok(cache) = self.view_cache.lock() else {
            return Vec::new();
        };
        let mut views = cache
            .iter()
            .filter(|(_, view)| view.task_id == task_id && view.user_id == user_id)
            .map(|(id, view)| {
                self.ensure_display_stream(id);
                let (frame_revision, frame_width, frame_height, frame_png, frame_error) = self
                    .display_streams
                    .lock()
                    .ok()
                    .and_then(|streams| streams.get(id).cloned())
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
                let mut view = view.clone();
                view.remaining_ms = view.expires_at_unix_ms.saturating_sub(unix_time_ms());
                view.frame_revision = frame_revision;
                view.frame_width = frame_width;
                view.frame_height = frame_height;
                view.frame_png = if view.private_input { None } else { frame_png };
                view.frame_error = frame_error;
                if let Ok(pending) = self.pending_controls.lock()
                    && let Some(error) = pending.get(id)
                {
                    view.control_transition_pending = error.is_none();
                    view.control_error = error.clone();
                }
                view
            })
            .collect::<Vec<_>>();
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
            resource_budget: session.policy.resource_budget.clone(),
            actions_remaining: session
                .policy
                .resource_budget
                .action_limit
                .saturating_sub(active.next_action_sequence.saturating_sub(1) as u32),
            lifecycle: session.lifecycle,
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
            action_log: active.action_log.iter().cloned().collect(),
        };
        if let Ok(mut views) = self.view_cache.lock() {
            views.insert(session.identity.session_id.clone(), view);
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

    fn ensure_display_stream(self: &Arc<Self>, session_id: &str) {
        let mut streams = match self.display_streams.lock() {
            Ok(streams) => streams,
            Err(_) => return,
        };
        if let Some(snapshot) = streams.get(session_id) {
            if let Ok(mut snapshot) = snapshot.lock() {
                snapshot.last_requested = Instant::now();
            }
            return;
        }
        let snapshot = Arc::new(Mutex::new(DisplaySnapshot {
            last_requested: Instant::now(),
            frame_revision: 0,
            frame_width: 0,
            frame_height: 0,
            frame_png: None,
            frame_error: false,
        }));
        streams.insert(session_id.to_owned(), snapshot.clone());
        let controller: Weak<Self> = Arc::downgrade(self);
        let key = session_id.to_owned();
        std::thread::spawn(move || {
            loop {
                let Some(controller) = controller.upgrade() else {
                    break;
                };
                let (session, handle, display_io, private_input, ready) =
                    match controller.sessions.lock() {
                        Ok(sessions) => match sessions.get(&key) {
                            Some(active) => (
                                active.session.clone(),
                                active.handle.clone(),
                                active.display_io.clone(),
                                active.private_input,
                                active.started && !active.paused && !active.uncertain,
                            ),
                            None => break,
                        },
                        Err(_) => break,
                    };
                let still_requested = snapshot
                    .lock()
                    .is_ok_and(|state| state.last_requested.elapsed() <= Duration::from_secs(4));
                if !still_requested {
                    if let Ok(mut streams) = controller.display_streams.lock()
                        && streams
                            .get(&key)
                            .is_some_and(|current| Arc::ptr_eq(current, &snapshot))
                    {
                        streams.remove(&key);
                    }
                    break;
                }
                if !ready || private_input {
                    if let Ok(mut state) = snapshot.lock() {
                        state.frame_png = None;
                    }
                } else {
                    let frame = match display_io.lock() {
                        Ok(_guard) => controller
                            .runtime
                            .as_ref()
                            .ok_or(crate::computer_use::ComputerBackendErrorV1::UnsupportedHost)
                            .and_then(|runtime| {
                                runtime.backend.capture_display_frame(
                                    &session,
                                    &handle,
                                    unix_time_ms(),
                                )
                            }),
                        Err(_) => Err(crate::computer_use::ComputerBackendErrorV1::Io),
                    };
                    let still_private = controller
                        .sessions
                        .lock()
                        .ok()
                        .and_then(|sessions| sessions.get(&key).map(|active| active.private_input))
                        .unwrap_or(true);
                    if let Ok(mut state) = snapshot.lock() {
                        match frame {
                            Ok(frame) if !still_private => {
                                state.frame_revision = state.frame_revision.saturating_add(1);
                                state.frame_width = frame.width;
                                state.frame_height = frame.height;
                                state.frame_png = Some(frame.png);
                                state.frame_error = false;
                            }
                            Ok(_) => state.frame_png = None,
                            Err(_) => state.frame_error = true,
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(750));
            }
        });
    }
}
