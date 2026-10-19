use super::{ResourceReservation, WorkerCapacityPool};
use std::collections::HashSet;

impl WorkerCapacityPool {
    pub(super) fn add_durable_reservations(
        &self,
        total: &mut ResourceReservation,
        active_ids: &HashSet<String>,
    ) -> anyhow::Result<HashSet<String>> {
        let owner = crate::persistence::execution_records::owner_instance_id_at(&self.state_root)
            .map_err(|_| anyhow::anyhow!("worker execution ownership is unavailable"))?;
        let records = crate::persistence::execution_records::unfinished_for_owner_at(
            &self.state_root,
            &owner,
        )
        .map_err(|_| anyhow::anyhow!("worker execution reservations are unavailable"))?;
        let mut durable_ids = HashSet::new();
        for record in records.into_iter().filter(|record| {
            (record.controller_target_id.as_deref() == Some(self.worker_id.as_str())
                || (self.worker_id == "local"
                    && record.target_id == "local"
                    && record.controller_target_id.is_none()))
                && !active_ids.contains(&record.execution_id)
        }) {
            durable_ids.insert(record.execution_id.clone());
            let request = record.request.ok_or_else(|| {
                anyhow::anyhow!("unfinished worker execution has no resource budget")
            })?;
            total.cpu_millis = total
                .cpu_millis
                .checked_add(request.resource_budget.cpu_millis)
                .ok_or_else(|| anyhow::anyhow!("worker CPU reservation overflow"))?;
            total.memory_bytes = total
                .memory_bytes
                .checked_add(request.resource_budget.memory_bytes)
                .ok_or_else(|| anyhow::anyhow!("worker memory reservation overflow"))?;
        }
        Ok(durable_ids)
    }
}
