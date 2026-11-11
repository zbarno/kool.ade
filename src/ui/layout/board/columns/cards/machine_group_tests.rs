//! Unit coverage for the parent machine-setup card's pure presentation
//! logic (markers, summaries, fix eligibility). Rendering itself rides the
//! standard egui layout pass.

use super::{Marker, describe_row, summarize};
use crate::persistence::setup_tasks::{LocalTask, State};

fn task(id: &str, state: State) -> LocalTask {
    LocalTask {
        schema_version: 1,
        id: id.into(),
        title: id.into(),
        explanation: String::new(),
        next_step: String::new(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state,
        created_at_unix_ms: 1,
        observed_at_unix_ms: 1,
        scan_generation: 7,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts: Vec::new(),
        events: Vec::new(),
        superseded_by: Vec::new(),
    }
}

#[test]
fn markers_track_the_operational_meaning_of_each_state() {
    assert_eq!(
        describe_row(&task("a", State::Verified)).marker,
        Marker::Complete
    );
    assert_eq!(
        describe_row(&task("b", State::InProgress)).marker,
        Marker::Working
    );
    assert_eq!(
        describe_row(&task("c", State::NeedsApproval)).marker,
        Marker::ConsentNeeded
    );
    assert_eq!(
        describe_row(&task("d", State::BlockedByPrerequisite)).marker,
        Marker::Barrier
    );
    assert_eq!(
        describe_row(&task("e", State::Failed)).marker,
        Marker::Problem
    );
    assert_eq!(
        describe_row(&task("f", State::Unsupported)).marker,
        Marker::Problem
    );
    assert_eq!(
        describe_row(&task("g", State::ReadyToFix)).marker,
        Marker::Incomplete
    );
}

#[test]
fn row_offers_a_fix_only_when_dispatch_can_act() {
    let ready = task("setup:computer:qemu", State::ReadyToFix);
    assert!(describe_row(&ready).offers_fix);
    let unrouted = task("some:unknown:step", State::ReadyToFix);
    assert!(!describe_row(&unrouted).offers_fix);
}

#[test]
fn summary_counts_verified_against_every_relevant_check() {
    let ok = task("ok", State::Verified);
    let todo = task("todo", State::ReadyToFix);
    let gone = task("gone", State::Superseded);
    let summary = summarize(&[&ok, &todo, &gone]);
    assert_eq!(summary.verified, 1);
    assert_eq!(summary.total, 2);
}
