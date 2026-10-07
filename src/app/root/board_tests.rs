use super::*;
#[path = "board_tests/support.rs"]
mod support;
pub(super) use support::*;
#[path = "board_tests/cancellation.rs"]
mod cancellation;
#[path = "board_tests/card_activity.rs"]
mod card_activity;
#[path = "board_tests/card_navigation.rs"]
mod card_navigation;
#[path = "board_tests/conversation_navigation.rs"]
mod conversation_navigation;
#[path = "board_tests/implementation_gates.rs"]
mod implementation_gates;
#[path = "board_tests/kanban_states.rs"]
mod kanban_states;
#[path = "board_tests/planning_decisions.rs"]
mod planning_decisions;
#[path = "board_tests/queue_execution.rs"]
mod queue_execution;
#[path = "board_tests/queue_recovery.rs"]
mod queue_recovery;
#[path = "board_tests/relationships.rs"]
mod relationships;
#[path = "board_tests/responsive_layout.rs"]
mod responsive_layout;
#[path = "board_tests/review_approval.rs"]
mod review_approval;
#[path = "board_tests/task_completion.rs"]
mod task_completion;
#[path = "board_tests/task_details.rs"]
mod task_details;
#[path = "board_tests/workspace_actions.rs"]
mod workspace_actions;

#[path = "board_tests/mockup_layout.rs"]
mod mockup_layout;

#[path = "board_tests/task_detail_design.rs"]
mod task_detail_design;

#[path = "board_tests/visual_gallery.rs"]
mod visual_gallery;
