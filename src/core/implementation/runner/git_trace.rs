use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

pub(crate) type GitWorktreeTrace = Arc<Mutex<Vec<Vec<String>>>>;

thread_local! {
    static TRACE: RefCell<Option<GitWorktreeTrace>> = const { RefCell::new(None) };
}

pub(crate) struct GitWorktreeTraceGuard(Option<GitWorktreeTrace>);

pub(crate) fn capture_git_worktree_commands() -> (GitWorktreeTrace, GitWorktreeTraceGuard) {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let previous = TRACE.with(|current| current.replace(Some(trace.clone())));
    (trace, GitWorktreeTraceGuard(previous))
}

pub(super) fn record(arguments: &[&str]) {
    if !arguments.contains(&"worktree") {
        return;
    }
    TRACE.with(|current| {
        if let Some(trace) = current.borrow().as_ref()
            && let Ok(mut trace) = trace.lock()
        {
            trace.push(
                arguments
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
            );
        }
    });
}

impl Drop for GitWorktreeTraceGuard {
    fn drop(&mut self) {
        TRACE.with(|current| {
            current.replace(self.0.take());
        });
    }
}
