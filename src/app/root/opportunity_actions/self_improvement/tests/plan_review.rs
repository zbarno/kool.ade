use super::*;
use crate::{
    app::root::Screen,
    core::{
        lifecycle::{FeaturePhase, LifecycleStore, plan_snapshot},
        opportunity::OpportunityStore,
        self_improvement::{SelfImprovementPhaseV1 as Phase, SelfImprovementStore},
    },
    domain::ArtifactIdentity,
    ui::Surface,
};
use std::time::{Duration, Instant};

#[path = "plan_review/app_flow.rs"]
mod app_flow;
#[path = "plan_review/fixture.rs"]
mod fixture;
#[path = "plan_review/reviewer.rs"]
mod reviewer;
#[path = "plan_review/task_generation.rs"]
mod task_generation;
