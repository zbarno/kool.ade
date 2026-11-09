use crate::artifacts::planning_store::PlanningStore;

mod file_flows;
mod filesystem;
mod lifecycle;
mod operations;
mod persistence;

pub(super) const BASE: &str = "computer-use/transfers/v1";
pub(super) const GRANTS: &str = "computer-use/transfers/v1/grants";
pub(super) const CLIPBOARD: &str = "computer-use/transfers/v1/clipboard";
pub(super) const ARTIFACTS: &str = "computer-use/transfers/v1/artifacts";
pub(super) const BYTES: &str = "computer-use/transfers/v1/blobs";

#[derive(Clone, Debug)]
pub struct ComputerArtifactBridgeV1 {
    pub(super) store: PlanningStore,
}

#[derive(Debug)]
pub enum ComputerArtifactBridgeErrorV1 {
    Denied,
    Invalid,
    Expired,
    StaleRevision,
    Quota,
    Storage,
}

impl std::fmt::Display for ComputerArtifactBridgeErrorV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Denied => "transfer is denied by controller policy",
            Self::Invalid => "transfer data is invalid or exceeds its bound",
            Self::Expired => "transfer grant is expired or revoked",
            Self::StaleRevision => "task source revision changed; transfer must be re-authorized",
            Self::Quota => "transfer storage quota is exhausted",
            Self::Storage => "transfer storage is unavailable",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ComputerArtifactBridgeErrorV1 {}
