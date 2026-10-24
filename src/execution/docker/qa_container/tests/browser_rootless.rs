use super::*;
use crate::{
    execution::docker::{
        images::{ImageLock, RuntimeProfile},
        qa_container::{
            QaDriverNetwork, QaWebPreviewSpec, cleanup_driver_container, cleanup_web_preview,
            create_driver_container, run_created_driver, run_created_driver_with_status,
            snapshot_selected_files, validate_browser_driver_network,
            validate_preview_relay_network,
        },
    },
    project_runtime::{ExecutionRecipeV1, NetworkRequirement, RecipeSource},
    runtime_qa::{
        QaBoundedLimitsV1, QaBrowserLocatorV1, QaDriverActionV1, QaHttpMethodV1, QaScenarioStepV1,
        QaScenarioV1, build_browser_request, build_service_request, parse_browser_output,
    },
};
use sha2::Digest as _;
use std::{collections::BTreeMap, time::Duration};

#[path = "browser_rootless/cancellation.rs"]
mod cancellation;
#[path = "browser_rootless/cleanup.rs"]
mod cleanup;
#[path = "browser_rootless/concurrency.rs"]
mod concurrency;
#[path = "browser_rootless/defects.rs"]
mod defects;
#[path = "browser_rootless/lifecycle.rs"]
mod lifecycle;
#[path = "browser_rootless/redirect.rs"]
mod redirect;
#[path = "browser_rootless/support.rs"]
mod support;
use support::step;

#[path = "browser_rootless/run.rs"]
mod run;
#[path = "browser_rootless/scenario.rs"]
mod scenario;
#[path = "browser_rootless/service_driver.rs"]
mod service_driver;
#[path = "browser_rootless/task_runtime.rs"]
mod task_runtime;
