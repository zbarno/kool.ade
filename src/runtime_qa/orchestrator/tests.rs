#[path = "tests/headless_service.rs"]
mod headless_service;
#[path = "tests/multi_scenario.rs"]
mod multi_scenario;
#[path = "tests/orchestration.rs"]
mod orchestration;
#[path = "tests/reviewer.rs"]
mod reviewer;
#[path = "tests/support.rs"]
mod support;

use super::*;
use support::{FakeDriver, fixture, reviewer_prompt};
