use crate::{
    execution::backend::BackendCapabilitiesV1,
    persistence::remote_workers::RemoteWorkerRegistration,
};

pub(super) struct ObservedWorker {
    pub(super) registration: RemoteWorkerRegistration,
    pub(super) capabilities: BackendCapabilitiesV1,
}
