use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppSurface {
    Web,
    LinuxDesktop,
    HeadlessService,
    UnsupportedTarget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetCapabilityState {
    Ready,
    MissingToolchain,
    MissingDependencies,
    UnsupportedPlatform,
    UnauthorizedRecipe,
    DriverUnavailable,
    BuildFailed,
    StartFailed,
    ReadyButUntested,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeOperation {
    Build,
    Test,
    Start,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeReceiptState {
    Running,
    Passed,
    Failed,
    NotApplicable,
}

mod manifest;
mod readiness;
mod receipt;

pub use manifest::{AppTargetV1, ProjectCapabilityManifestV1};
pub use readiness::ReadinessProbeV1;
pub use receipt::RuntimeReceiptV1;
