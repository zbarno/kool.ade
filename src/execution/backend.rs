use crate::execution::{
    contract::{
        EventPageRequestV1, ExecutionEventV1, ExecutionRequestV1, ExecutionResultV1,
        ExecutionTarget, WorkspaceRef,
    },
    errors::ExecutionError,
};
use std::{future::Future, pin::Pin};

pub type BackendFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ExecutionError>> + Send + 'a>>;

/// Shared lifecycle API for local and remote Docker workers.
/// Implementations resolve local paths internally; request messages never carry them.
pub trait ExecutionBackend: Send + Sync {
    fn probe(&self, target: &ExecutionTarget) -> BackendFuture<'_, BackendProbeV1>;

    fn prepare_workspace<'a>(
        &'a self,
        workspace: &'a WorkspaceRef,
        target: &'a ExecutionTarget,
    ) -> BackendFuture<'a, WorkspaceLeaseV1>;

    fn start<'a>(&'a self, request: &'a ExecutionRequestV1)
    -> BackendFuture<'a, ExecutionHandleV1>;

    fn exec<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()>;

    fn events<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        page: EventPageRequestV1,
    ) -> BackendFuture<'a, Vec<ExecutionEventV1>>;

    fn cancel<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        cancellation_generation: u64,
    ) -> BackendFuture<'a, ()>;

    fn collect_artifacts<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ExecutionResultV1>;

    fn release<'a>(&'a self, handle: &'a ExecutionHandleV1) -> BackendFuture<'a, ()>;
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BackendProbeV1 {
    pub wire_version: u16,
    pub available: bool,
    pub compatible: bool,
    pub failure: Option<crate::execution::errors::ExecutionErrorKind>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLeaseV1 {
    pub lease_id: String,
    pub workspace: WorkspaceRef,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionHandleV1 {
    pub execution_id: String,
    pub target: ExecutionTarget,
    pub request_sequence: u64,
}
