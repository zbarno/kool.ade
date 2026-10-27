use serde::Serialize;

mod runtime;
mod service;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProbeOutcome {
    Ready,
    Conflict,
    Unknown,
}

impl ProbeOutcome {
    pub(super) fn ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct EndpointObservation {
    pub trusted: bool,
    pub fingerprint: Option<String>,
}

pub(super) fn current_endpoint() -> EndpointObservation {
    runtime::current_endpoint()
}

pub(super) fn rootlesskit_smoke() -> ProbeOutcome {
    runtime::rootlesskit_smoke()
}

pub(super) fn service_absent() -> ProbeOutcome {
    service::service_absent()
}
