use super::board_tests::{click_text, fixture, frame, text_position};
use super::*;
use std::sync::{Arc, Mutex};

#[path = "conversation_tests/support.rs"]
mod support;
use support::*;

#[cfg(test)]
#[path = "conversation_tests/setup_failure.rs"]
mod setup_failure;

#[cfg(test)]
#[path = "conversation_tests/new_task.rs"]
mod new_task;

#[cfg(test)]
#[path = "conversation_tests/dogfood.rs"]
mod dogfood;

#[path = "conversation_tests/archive_controls.rs"]
mod archive_controls;
#[path = "conversation_tests/discussion_workflow.rs"]
mod discussion_workflow;
#[path = "conversation_tests/message_hierarchy.rs"]
mod message_hierarchy;
#[path = "conversation_tests/new_task_navigation.rs"]
mod new_task_navigation;
#[path = "conversation_tests/parallel_task_chats.rs"]
mod parallel_task_chats;
#[path = "conversation_tests/readable_replies.rs"]
mod readable_replies;
#[path = "conversation_tests/reply_history.rs"]
mod reply_history;
#[path = "conversation_tests/reply_outcomes.rs"]
mod reply_outcomes;
#[path = "conversation_tests/stranded_reply.rs"]
mod stranded_reply;
