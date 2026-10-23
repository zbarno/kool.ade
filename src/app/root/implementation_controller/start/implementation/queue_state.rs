use crate::{
    app::session::Project,
    core::{implementation::ImplementationStatus, implementation_queue::Queue},
};

pub(super) struct QueueStart {
    pub(super) previous_queue: Queue,
    pub(super) task_id: Option<String>,
    pub(super) wait_ms: Option<u64>,
    pub(super) resuming: bool,
}

pub(super) fn begin(project: &mut Project, ticket: &str, manual: bool) -> Option<QueueStart> {
    if project.queue_lock.is_none() {
        match Queue::acquire(&project.state.repo_root) {
            Ok(lock) => project.queue_lock = Some(lock),
            Err(error) => {
                project.queue.last_error = error.to_string();
                return None;
            }
        }
    }
    let previous_queue = project.queue.clone();
    let (task_id, wait_ms) = super::super::queue_wait::take(
        project,
        ticket,
        crate::persistence::harness_settings::current_unix_ms(),
    );
    let resuming = project
        .implementation_states
        .get(ticket)
        .is_some_and(|state| {
            matches!(
                state.status,
                ImplementationStatus::Blocked | ImplementationStatus::Interrupted
            )
        });
    if project.queue.auto_build {
        project.queue.running = true;
        project.queue.recovery_paused = false;
    }
    if manual {
        project.queue.recovery_attempts.remove(ticket);
        project.queue.recovery_paused = false;
    }
    project.queue.in_flight.insert(ticket.to_owned());
    project.queue.blocked.remove(ticket);
    project.queue.last_error.clear();
    if let Err(error) = project.queue.save(&project.state.repo_root) {
        project.queue = previous_queue;
        project.queue.running = false;
        project.queue.last_error = error.to_string();
        if project.active_implementations.is_empty() {
            project.queue_lock = None;
        }
        return None;
    }
    Some(QueueStart {
        previous_queue,
        task_id,
        wait_ms,
        resuming,
    })
}
