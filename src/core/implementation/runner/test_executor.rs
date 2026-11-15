use super::*;
use std::{
    cell::RefCell,
    sync::{Mutex, OnceLock},
};

/// Test-only command executor installed by the isolated implementation sandbox.
/// Production builds have no executor slot and always use the Docker backend.
type TestVerificationExecutor = std::sync::Arc<
    dyn Fn(&Path, &str, &crate::execution::contract::WorkspaceRef) -> anyhow::Result<String>
        + Send
        + Sync,
>;

thread_local! {
    static TEST_VERIFICATION_EXECUTOR: RefCell<Option<TestVerificationExecutor>> =
        const { RefCell::new(None) };
}

type ScopedTestVerificationExecutor = std::sync::Arc<
    dyn Fn(&Path, &str, &crate::execution::contract::WorkspaceRef) -> anyhow::Result<String>
        + Send
        + Sync,
>;

type ScopedTestVerificationExecutorEntry = (u64, PathBuf, String, ScopedTestVerificationExecutor);

static SCOPED_TEST_VERIFICATION_EXECUTORS: OnceLock<
    Mutex<Vec<ScopedTestVerificationExecutorEntry>>,
> = OnceLock::new();
static NEXT_SCOPED_TEST_VERIFICATION_EXECUTOR: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

pub(crate) struct ScopedTestVerificationExecutorGuard(u64);

pub(crate) fn install_scoped_test_verification_executor(
    root: PathBuf,
    task_uid: String,
    executor: impl Fn(&Path, &str, &crate::execution::contract::WorkspaceRef) -> anyhow::Result<String>
    + Send
    + Sync
    + 'static,
) -> ScopedTestVerificationExecutorGuard {
    let id = NEXT_SCOPED_TEST_VERIFICATION_EXECUTOR.fetch_add(1, Ordering::Relaxed);
    SCOPED_TEST_VERIFICATION_EXECUTORS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("test verification executor registry is not poisoned")
        .push((id, root, task_uid, std::sync::Arc::new(executor)));
    ScopedTestVerificationExecutorGuard(id)
}

pub(crate) fn scoped_test_verification_executor(
    cwd: &Path,
    command: &str,
    workspace: &crate::execution::contract::WorkspaceRef,
) -> Option<anyhow::Result<String>> {
    let executors = SCOPED_TEST_VERIFICATION_EXECUTORS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .ok()?;
    let executor = executors
        .iter()
        .filter(|(_, root, task_uid, _)| cwd.starts_with(root) && workspace.task_id == *task_uid)
        .max_by_key(|(_, root, _, _)| root.components().count())
        .map(|(_, _, _, executor)| executor.clone())?;
    Some(executor(cwd, command, workspace))
}

impl Drop for ScopedTestVerificationExecutorGuard {
    fn drop(&mut self) {
        if let Some(executors) = SCOPED_TEST_VERIFICATION_EXECUTORS.get() {
            executors
                .lock()
                .expect("test verification executor registry is not poisoned")
                .retain(|(id, _, _, _)| *id != self.0);
        }
    }
}

pub(crate) struct TestVerificationExecutorGuard {
    previous: Option<TestVerificationExecutor>,
}

pub(crate) fn install_test_verification_executor(
    executor: impl Fn(&Path, &str, &crate::execution::contract::WorkspaceRef) -> anyhow::Result<String>
    + Send
    + Sync
    + 'static,
) -> TestVerificationExecutorGuard {
    let previous =
        TEST_VERIFICATION_EXECUTOR.with(|slot| slot.replace(Some(std::sync::Arc::new(executor))));
    TestVerificationExecutorGuard { previous }
}

pub(crate) fn test_verification_executor() -> Option<TestVerificationExecutor> {
    TEST_VERIFICATION_EXECUTOR.with(|slot| slot.borrow().clone())
}

impl Drop for TestVerificationExecutorGuard {
    fn drop(&mut self) {
        TEST_VERIFICATION_EXECUTOR.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}
