use super::*;
use crate::{
    app::root::board_tests::click_text,
    artifacts::planning_store::StoreMode,
    core::opportunity::{
        EvidenceConfidentialityV1, OpportunityDecisionKindV1, OpportunityEvidenceRefV1,
        OpportunityKindV1, OpportunityOriginV1, OpportunitySaveResult, OpportunityScopeV1,
        OpportunityStatusV1, OpportunityTargetV1, OpportunityV1,
    },
    harness::prompt_catalog::{OpportunityChoice, OpportunityDecisionV1},
};

#[path = "tests/authorization.rs"]
mod authorization;
#[path = "tests/clarification.rs"]
mod clarification;
#[path = "tests/installation.rs"]
mod installation;
#[path = "tests/plan_review.rs"]
mod plan_review;
#[path = "tests/pursue.rs"]
mod pursue;
#[path = "tests/retry_recovery.rs"]
mod retry_recovery;
#[path = "tests/run_creation.rs"]
mod run_creation;
#[path = "tests/test_support.rs"]
mod test_support;

use test_support::{app_product_proposal, app_proposal};
