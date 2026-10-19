use std::collections::BTreeMap;

use super::{ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1};

const MAX_EVENT_REPLAY_WINDOW: u64 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventAcceptance {
    Accepted,
    Duplicate,
}

#[derive(Default)]
pub struct EventStreamValidator {
    execution_id: Option<String>,
    seen: BTreeMap<u64, ExecutionEventV1>,
    next_sequence: u64,
    lifecycle: Option<ExecutionLifecycleV1>,
    last_timestamp_unix_ms: u64,
}

impl EventStreamValidator {
    pub fn accept(&mut self, event: &ExecutionEventV1) -> anyhow::Result<EventAcceptance> {
        event.validate()?;
        if let Some(execution_id) = &self.execution_id {
            anyhow::ensure!(
                execution_id == &event.execution_id,
                "event execution ID changed"
            );
        }
        if let Some(previous) = self.seen.get(&event.sequence) {
            anyhow::ensure!(
                previous == event,
                "event sequence was reused with different data"
            );
            return Ok(EventAcceptance::Duplicate);
        }
        anyhow::ensure!(
            event.sequence >= self.next_sequence,
            "event replay is older than the deduplication window"
        );
        anyhow::ensure!(
            event.sequence == self.next_sequence.max(1),
            "event sequence is not contiguous"
        );
        let next_sequence = event
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("event sequence overflow"))?;
        anyhow::ensure!(
            event.timestamp_unix_ms >= self.last_timestamp_unix_ms,
            "event timestamp moved backwards"
        );
        self.validate_order(&event.event)?;
        if self.execution_id.is_none() {
            self.execution_id = Some(event.execution_id.clone());
        }
        self.last_timestamp_unix_ms = event.timestamp_unix_ms;
        self.seen.insert(event.sequence, event.clone());
        self.next_sequence = next_sequence;
        let first_retained = self.next_sequence.saturating_sub(MAX_EVENT_REPLAY_WINDOW);
        self.seen.retain(|sequence, _| *sequence >= first_retained);
        Ok(EventAcceptance::Accepted)
    }

    fn validate_order(&mut self, event: &ExecutionEventKindV1) -> anyhow::Result<()> {
        use ExecutionEventKindV1 as Event;
        if self.lifecycle.is_none() {
            anyhow::ensure!(
                matches!(
                    event,
                    Event::Lifecycle {
                        state: ExecutionLifecycleV1::Queued
                    }
                ),
                "execution event stream must start at queued"
            );
            self.lifecycle = Some(ExecutionLifecycleV1::Queued);
            return Ok(());
        }
        let Some(current) = self.lifecycle else {
            unreachable!()
        };
        anyhow::ensure!(
            !current.terminal(),
            "event appeared after a terminal lifecycle state"
        );
        match event {
            Event::Lifecycle { state } => {
                let allowed = matches!(
                    (current, state),
                    (
                        ExecutionLifecycleV1::Queued,
                        ExecutionLifecycleV1::Preparing
                    ) | (
                        ExecutionLifecycleV1::Preparing,
                        ExecutionLifecycleV1::WaitingForDependencyApproval
                    ) | (
                        ExecutionLifecycleV1::Preparing,
                        ExecutionLifecycleV1::WaitingForRuntime
                    ) | (
                        ExecutionLifecycleV1::WaitingForDependencyApproval,
                        ExecutionLifecycleV1::Preparing
                    ) | (
                        ExecutionLifecycleV1::WaitingForRuntime,
                        ExecutionLifecycleV1::Preparing
                    ) | (
                        ExecutionLifecycleV1::WaitingForDependencyApproval,
                        ExecutionLifecycleV1::WaitingForRuntime
                    ) | (
                        ExecutionLifecycleV1::WaitingForRuntime,
                        ExecutionLifecycleV1::WaitingForDependencyApproval
                    ) | (
                        ExecutionLifecycleV1::WaitingForDependencyApproval,
                        ExecutionLifecycleV1::Running
                    ) | (
                        ExecutionLifecycleV1::Running,
                        ExecutionLifecycleV1::WaitingForDependencyApproval
                    ) | (
                        ExecutionLifecycleV1::Preparing,
                        ExecutionLifecycleV1::Running
                    ) | (
                        ExecutionLifecycleV1::Running,
                        ExecutionLifecycleV1::Collecting
                    ) | (_, ExecutionLifecycleV1::Failed)
                        | (_, ExecutionLifecycleV1::Cancelled)
                        | (_, ExecutionLifecycleV1::Interrupted)
                        | (
                            ExecutionLifecycleV1::Collecting,
                            ExecutionLifecycleV1::Succeeded
                        )
                );
                anyhow::ensure!(allowed, "invalid execution lifecycle transition");
                self.lifecycle = Some(*state);
            }
            Event::Stdout { .. } | Event::Stderr { .. } => anyhow::ensure!(
                current == ExecutionLifecycleV1::Running,
                "output is only valid while execution is running"
            ),
            Event::Metrics { .. } => anyhow::ensure!(
                matches!(
                    current,
                    ExecutionLifecycleV1::Running | ExecutionLifecycleV1::Collecting
                ),
                "resource metrics arrived outside execution"
            ),
            Event::Artifact { .. } => anyhow::ensure!(
                current == ExecutionLifecycleV1::Collecting,
                "artifacts are only valid while collecting"
            ),
        }
        Ok(())
    }
}
