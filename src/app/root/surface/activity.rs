macro_rules! activity_surface_methods {
    () => {
        fn activity_samples(&self, key: Option<&str>) -> Vec<(i64, u64)> {
            self.activity_samples_for_surface(key)
        }

        fn activity_active(&self, key: &str) -> bool {
            self.implementation_active(key)
                || self.task_chat_active(key)
                || matches!(&self.screen,
                Screen::Connected(p) if p.investigation.as_ref().is_some_and(|run| run.item_id == key))
        }

        fn is_busy(&self) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some() || !p.active_implementations.is_empty())
        }

        fn task_progress(&self, ticket: &str) -> Option<&crate::harness::LiveProgress> {
            match &self.screen {
                Screen::Connected(p) => p.activity.tasks.get(ticket),
                _ => None,
            }
        }

        fn max_parallel_tasks(&self) -> usize {
            match &self.screen {
                Screen::Connected(p) => p.queue.max_parallel.clamp(1, 8),
                _ => 3,
            }
        }

        fn active_task_count(&self) -> usize {
            match &self.screen {
                Screen::Connected(p) => p.active_implementations.len(),
                _ => 0,
            }
        }

        fn live_progress(&self) -> Option<&crate::harness::LiveProgress> {
            match &self.screen {
                Screen::Connected(p)
                    if p.task_chats.active.is_none()
                        && (p.active_turn.is_some() || p.activity.manager.is_some()) =>
                {
                    Some(&p.live_progress)
                }
                _ => None,
            }
        }

        fn implementation_state(
            &self,
            ticket: &str,
        ) -> Option<&crate::core::implementation::Implementation> {
            match &self.screen {
                Screen::Connected(p) => p.implementation_states.get(ticket),
                _ => None,
            }
        }

        fn implementation_failure(&self, ticket: &str) -> Option<&str> {
            match &self.screen {
                Screen::Connected(p) => p
                    .queue
                    .blocked
                    .get(ticket)
                    .map(|failure| failure.message.as_str()),
                _ => None,
            }
        }

        fn implementation_recovery(
            &self,
            ticket: &str,
        ) -> Option<crate::core::implementation::RecoveryDisposition> {
            match &self.screen {
                Screen::Connected(project) => project
                    .queue
                    .blocked
                    .get(ticket)
                    .map(|failure| failure.recovery),
                _ => None,
            }
        }

        fn task_detail_view(&mut self, ticket: &str) -> Option<crate::ui::task_detail::ViewModel> {
            self.task_detail_for_surface(ticket)
        }

        fn implementation_active(&self, ticket: &str) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.active_implementations.contains_key(ticket))
        }

        fn implementation_waiting_for_capacity(&self, ticket: &str) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.queue.waiting_for_capacity.contains(ticket))
        }

        fn implementation_elapsed(&self, ticket: &str) -> Option<String> {
            let Screen::Connected(project) = &self.screen else {
                return None;
            };
            let started = project.activity.tasks.get(ticket)?.telemetry.started_ms?;
            let seconds = (chrono::Utc::now().timestamp_millis() - started).max(0) / 1000;
            Some(format!("{}m {:02}s", seconds / 60, seconds % 60))
        }

        fn auto_plan(&self) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.queue.auto_plan)
        }

        fn auto_build(&self) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.queue.auto_build)
        }

        fn auto_implement(&self) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.queue.auto_build)
        }

        fn auto_publish(&self) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.queue.auto_publish)
        }

        fn require_independent_checks(&self) -> bool {
            matches!(&self.screen, Screen::Connected(project) if project.queue.require_independent_checks)
        }

        fn queue_status(&self) -> &str {
            match &self.screen {
                Screen::Connected(p) if !p.queue.last_error.is_empty() => &p.queue.last_error,
                Screen::Connected(p) if !p.queue.waiting_for_capacity.is_empty() => {
                    "Ready tasks are queued while all implementation slots are occupied"
                }
                Screen::Connected(p) if p.queue.running => "Automatic build queue is running",
                _ => "",
            }
        }

        fn stale_task_claim(&self) -> Option<(String, crate::core::task_claim::ClaimRecord)> {
            match &self.screen {
                Screen::Connected(project) => project.queue.stale_claim.clone(),
                Screen::Welcome => None,
            }
        }
    };
}

pub(super) use activity_surface_methods;
