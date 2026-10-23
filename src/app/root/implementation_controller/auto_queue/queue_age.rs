use crate::{app::session::Project, core::implementation_queue::Queue};

pub(super) fn clear_ready_wait_ages(project: &mut Project) {
    let previous = project.queue.clone();
    if !project.queue.clear_ready_waits() {
        return;
    }
    let acquired = project.queue_lock.is_none();
    if acquired {
        match Queue::acquire(&project.state.repo_root) {
            Ok(lock) => project.queue_lock = Some(lock),
            Err(error) => {
                project.queue = previous;
                project.queue.last_error = format!("Cannot reset stale task queue age: {error}");
                return;
            }
        }
    }
    if let Err(error) = project.queue.save(&project.state.repo_root) {
        project.queue = previous;
        project.queue.last_error = format!("Cannot reset stale task queue age: {error}");
        if acquired {
            project.queue_lock = None;
        }
        return;
    }
    if acquired && project.active_implementations.is_empty() {
        project.queue_lock = None;
    }
}
