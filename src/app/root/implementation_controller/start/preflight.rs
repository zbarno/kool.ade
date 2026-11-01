use super::*;

impl KooladeApp {
    pub(super) fn preflight_task_start(
        &mut self,
        ticket: &str,
        manual: bool,
        capabilities: &crate::harness::runtime_capabilities::RuntimeCapabilities,
    ) -> bool {
        if self.reject_ambiguous_task_start(ticket, manual) {
            return true;
        }
        if matches!(&self.screen, Screen::Connected(project)
            if project.active_implementations.contains_key(ticket)
                || project.active_implementations.len()
                    >= project.queue.max_parallel.clamp(1, 8))
        {
            return true;
        }
        if capabilities.implementation {
            return false;
        }
        let message = capabilities.implementation_unavailable_message().to_owned();
        if let Screen::Connected(project) = &mut self.screen {
            project.queue.last_error = message.clone();
        }
        self.toasts.warning(message);
        true
    }
}
