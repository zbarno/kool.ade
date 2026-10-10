use super::*;
mod telemetry;

impl KooladeApp {
    pub(super) fn activity_samples_for_surface(&self, key: Option<&str>) -> Vec<(i64, u64)> {
        let Screen::Connected(p) = &self.screen else {
            return Vec::new();
        };
        if key.is_none()
            && let Some(overall) = &p.activity.overall
        {
            return overall.telemetry.samples.clone();
        }
        let item_id = key
            .and_then(|key| {
                p.state
                    .items
                    .iter()
                    .chain(&p.state.resolved_items)
                    .find(|item| item.conversation_key() == key)
                    .map(|item| item.id.as_str())
            })
            .or(key);
        let mut buckets = std::collections::BTreeMap::<i64, u64>::new();
        for (id, progress) in &p.activity.tasks {
            if item_id.is_none_or(|key| key == id) {
                for (bucket, count) in &progress.telemetry.samples {
                    *buckets.entry(*bucket).or_default() += count;
                }
            }
        }
        for (id, progress) in &p.activity.conversations {
            if key.is_none_or(|key| key == id) {
                for (bucket, count) in &progress.telemetry.samples {
                    *buckets.entry(*bucket).or_default() += count;
                }
            }
        }
        buckets.into_iter().collect()
    }
    pub(super) fn task_detail_for_surface(
        &mut self,
        ticket: &str,
    ) -> Option<crate::ui::task_detail::ViewModel> {
        let (
            implementation,
            implementation_active,
            queued_failure,
            failure_disposition,
            messages,
            progress,
            conversation_active,
            conversation_error,
            task_status,
            draft,
            auto_build,
            can_start,
        ) = match &self.screen {
            Screen::Connected(project) => (
                project.implementation_states.get(ticket).cloned(),
                project.active_implementations.contains_key(ticket),
                project
                    .queue
                    .blocked
                    .get(ticket)
                    .map(|failure| failure.message.clone()),
                project
                    .queue
                    .blocked
                    .get(ticket)
                    .map(|failure| failure.recovery),
                project
                    .task_chats
                    .messages
                    .get(ticket)
                    .cloned()
                    .unwrap_or_default(),
                project.activity.tasks.get(ticket).cloned(),
                project.task_turns.contains_key(ticket),
                project.task_chats.error.clone(),
                project
                    .task_documents
                    .iter()
                    .find(|document| document.path == ticket)
                    .and_then(|document| document.task_state.as_ref())
                    .map(|state| state.status),
                project
                    .task_chats
                    .drafts
                    .get(ticket)
                    .cloned()
                    .unwrap_or_default(),
                project.queue.auto_build,
                project.active_implementations.len() < project.queue.max_parallel.clamp(1, 8),
            ),
            Screen::Welcome => return None,
        };
        let failure = queued_failure.or_else(|| {
            (!implementation_active)
                .then(|| {
                    implementation
                        .as_ref()
                        .filter(|record| {
                            record.status
                                == crate::core::implementation::ImplementationStatus::Blocked
                        })
                        .map(|record| record.detail.clone())
                })
                .flatten()
        });
        let (implementation_metrics, feature_metrics) = match &self.screen {
            Screen::Connected(project) => telemetry::reports(project, ticket),
            Screen::Welcome => (None, None),
        };
        let attention = failure
            .as_deref()
            .and_then(|detail| self.attention_view(ticket, detail));
        let base = if implementation_active {
            crate::core::implementation::board_column(implementation.as_ref(), true)
        } else if let Some(status) = task_status {
            status.board_column()
        } else if failure.is_some() {
            3
        } else {
            crate::core::implementation::board_column(implementation.as_ref(), false)
        };
        let board_column = if implementation_active {
            base
        } else {
            crate::ui::task_chat::board_column(base, &messages, conversation_active)
        };
        Some(crate::ui::task_detail::ViewModel {
            implementation,
            implementation_active,
            failure,
            failure_disposition,
            attention,
            messages,
            progress,
            conversation_active,
            conversation_error,
            task_status,
            board_column,
            can_start,
            auto_build,
            draft,
            activity_samples: self.activity_samples(Some(ticket)),
            activity_active: self.activity_active(ticket),
            implementation_metrics,
            feature_metrics,
        })
    }
    pub(super) fn planning_board_for_surface(&self) -> crate::ui::planning_board::ViewModel {
        match &self.screen {
            Screen::Connected(p) => {
                let mut planning_work =
                    crate::core::planning_work::cards(&p.state, &p.planning_work);
                if let Some(active_key) = p
                    .active_turn
                    .as_ref()
                    .and(p.active_planning_work.as_deref())
                    && let Some(work) = planning_work.iter_mut().find(|work| work.key == active_key)
                {
                    work.status = crate::core::planning_work::WorkStatus::InProgress;
                }
                let mut planning_items = p
                    .state
                    .items
                    .iter()
                    .chain(&self.synth)
                    .chain(&p.state.resolved_items)
                    .cloned()
                    .collect::<Vec<_>>();
                planning_items.sort_by_key(|item| (item.priority.rank(), item.id.clone()));
                planning_items.dedup_by(|a, b| a.id == b.id);
                let eligible_item_ids = crate::core::routing::eligible_items(
                    &planning_items,
                    &self.cached_user,
                    &p.state.config.stakeholders,
                )
                .into_iter()
                .map(|item| item.id.clone())
                .collect();
                crate::ui::planning_board::ViewModel {
                    task_documents: p.task_documents.clone(),
                    planning_work,
                    planning_items,
                    setup_attention: self.setup_attention.clone(),
                    setup_checking: self.setup_probe.is_some(),
                    eligible_item_ids,
                    archived: p.archived_tasks.clone(),
                    cancelled: p.cancelled_work.clone(),
                }
            }
            _ => crate::ui::planning_board::ViewModel::default(),
        }
    }
}
