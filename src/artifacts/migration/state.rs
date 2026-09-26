//! Migrate private task-keyed state after its repository task paths are known.
mod implementation;
mod queue;

use std::{collections::BTreeMap, path::Path};

use super::plan::Plan;

pub(super) struct PrivatePlan {
    queue: queue::QueuePlan,
    implementations: implementation::MovePlan,
}

impl PrivatePlan {
    pub(super) fn build(
        repo: &Path,
        common: &Path,
        plan: &Plan,
        task_uids: &BTreeMap<String, String>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            queue: queue::QueuePlan::build(repo, common, plan)?,
            implementations: implementation::MovePlan::build(repo, common, plan, task_uids)?,
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.implementations.is_empty()
    }

    pub(super) fn apply(self) -> anyhow::Result<()> {
        self.implementations.apply()?;
        self.queue.apply()?;
        Ok(())
    }
}
