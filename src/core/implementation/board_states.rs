use super::{Implementation, ImplementationStatus, PullRequestState, load_all};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn load_board_states(repo: &Path) -> BTreeMap<String, Implementation> {
    let workflow = crate::artifacts::task_docs::load_workflow(repo).unwrap_or_default();
    let documents = crate::artifacts::task_docs::load_board(repo, &workflow);
    let mut states = load_all(repo);
    states.sort_by(|left, right| left.ticket.cmp(&right.ticket));
    let mut assigned = BTreeSet::new();
    let mut board = BTreeMap::new();

    for document in documents {
        let uid = document
            .identity
            .as_ref()
            .map(|identity| identity.uid.as_str());
        let matches = states
            .iter()
            .enumerate()
            .filter(|(index, state)| {
                !assigned.contains(index)
                    && uid.is_some_and(|uid| state.task_uid.as_deref() == Some(uid))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let selected = match matches.as_slice() {
            [index] => Some(*index),
            [] => states.iter().enumerate().find_map(|(index, state)| {
                (!assigned.contains(&index)
                    && state.ticket == document.path
                    && (uid.is_none() || state.task_uid.is_none()))
                .then_some(index)
            }),
            _ => None,
        };
        let Some(index) = selected else {
            continue;
        };
        assigned.insert(index);
        let mut state = states[index].clone();
        state.ticket = document.path.clone();
        if state.task_uid.is_none() {
            state.task_uid = uid.map(str::to_owned);
        }
        board.insert(document.path, state);
    }

    for state in states.into_iter().enumerate() {
        if assigned.contains(&state.0) {
            continue;
        }
        board.entry(state.1.ticket.clone()).or_insert(state.1);
    }
    board
}

pub const BOARD_COLUMNS: [&str; 5] = [
    "To do",
    "In progress",
    "In review",
    "Needs attention",
    "Done",
];
pub fn board_column(state: Option<&Implementation>, busy: bool) -> usize {
    let Some(state) = state else {
        return if busy { 1 } else { 0 };
    };
    match state.pr_state {
        Some(PullRequestState::Merged) => return 4,
        Some(PullRequestState::Closed) => return 3,
        _ => {}
    }
    match state.status {
        ImplementationStatus::Completed => 4,
        ImplementationStatus::AwaitingReview => 2,
        ImplementationStatus::Preparing
        | ImplementationStatus::Implementing
        | ImplementationStatus::Verifying
        | ImplementationStatus::ReadyToPublish
        | ImplementationStatus::Publishing
        | ImplementationStatus::WaitingToMerge
            if busy =>
        {
            1
        }
        _ => 3,
    }
}
