use std::{
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

use super::super::{ComputerUseMcpControllerV1, DisplaySnapshot};
use crate::computer_use::mcp::controller::unix_time_ms;

impl ComputerUseMcpControllerV1 {
    pub(super) fn ensure_display_stream(self: &Arc<Self>, session_id: &str) {
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
