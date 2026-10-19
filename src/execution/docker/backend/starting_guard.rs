use std::{collections::HashMap, sync::Mutex};

pub(in crate::execution::docker) struct StartingGuard<'a> {
    pub(super) starting: &'a Mutex<HashMap<String, (u32, u64)>>,
    pub(super) execution_id: String,
    pub(super) host_capacity_lease: Option<crate::execution::remote::WorkerCapacityLease>,
}

impl StartingGuard<'_> {
    pub(super) fn take_host_capacity_lease(
        &mut self,
    ) -> Option<crate::execution::remote::WorkerCapacityLease> {
        self.host_capacity_lease.take()
    }
}

impl Drop for StartingGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut starting) = self.starting.lock() {
            starting.remove(&self.execution_id);
        }
    }
}
