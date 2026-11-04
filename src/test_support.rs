//! Shared synchronization for tests that mutate process-wide environment state.

pub(crate) fn current_change_fixture(
    markdown: &str,
    status: crate::domain::ChangeStatus,
) -> anyhow::Result<String> {
    let heading = markdown
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .ok_or_else(|| anyhow::anyhow!("Change fixture has no title heading"))?;
    let (display_id, title) = heading
        .split_once(": ")
        .ok_or_else(|| anyhow::anyhow!("Change fixture title has no display ID"))?;
    let previous = crate::domain::ArtifactIdentity::from_markdown(markdown)?;
    let identified = crate::domain::ArtifactIdentity::preserve_markdown(
        markdown,
        previous.as_ref().map(|_| markdown),
        display_id,
        title,
    )?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)?
        .ok_or_else(|| anyhow::anyhow!("Change fixture identity was not written"))?;
    crate::domain::ChangeMetadata::write_markdown(&identified, &identity, status)
}

#[cfg(test)]
pub(crate) fn approved_git_intent(
    repository_id: &str,
    source_ref: &str,
    source_sha: &str,
    destination_ref: &str,
) -> crate::core::git_intent::GitIntentV1 {
    crate::core::git_intent::GitIntentV1::approve(
        repository_id.into(),
        source_ref.into(),
        source_sha.into(),
        destination_ref.into(),
        crate::core::git_intent::PublicationModeV1::HoldForReview,
        None,
    )
    .expect("test Git intent is valid")
}

use std::{
    cell::RefCell,
    ffi::{OsStr, OsString},
    marker::PhantomData,
    path::Path,
    sync::{Mutex, MutexGuard},
};

#[cfg(test)]
pub(crate) mod locked_images;

static PROCESS_ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    static PROCESS_ENVIRONMENT_LOCK_STATE: RefCell<Option<(usize, MutexGuard<'static, ()>)>> = const { RefCell::new(None) };
}

pub(crate) fn lock_process_environment() -> ProcessEnvironmentLock {
    let already_held = PROCESS_ENVIRONMENT_LOCK_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some((depth, _)) = state.as_mut() {
            *depth += 1;
            true
        } else {
            false
        }
    });
    if !already_held {
        let guard = PROCESS_ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        PROCESS_ENVIRONMENT_LOCK_STATE.with(|state| *state.borrow_mut() = Some((1, guard)));
    }
    ProcessEnvironmentLock {
        _not_send: PhantomData,
    }
}

#[cfg(test)]
pub(crate) fn set_test_origin_default(repository: &Path) {
    use std::process::Command;

    fn git(repository: &Path, arguments: &[&str]) -> std::process::Output {
        Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .output()
            .expect("Git is available for the local test fixture")
    }

    let remotes = git(repository, &["remote"]);
    assert!(remotes.status.success());
    if !String::from_utf8_lossy(&remotes.stdout)
        .lines()
        .any(|remote| remote == "origin")
    {
        return;
    }
    let existing_default = git(
        repository,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    );
    if existing_default.status.success() {
        let remote_branch = String::from_utf8(existing_default.stdout)
            .expect("Git ref is UTF-8")
            .trim()
            .to_owned();
        let local_branch = remote_branch.replacen("refs/remotes/origin/", "refs/heads/", 1);
        if git(
            repository,
            &["show-ref", "--verify", "--quiet", &local_branch],
        )
        .status
        .success()
        {
            assert!(
                git(repository, &["update-ref", &remote_branch, &local_branch])
                    .status
                    .success()
            );
        }
        return;
    }

    let main = "refs/heads/main";
    let branch = if git(repository, &["show-ref", "--verify", "--quiet", main])
        .status
        .success()
    {
        main.to_owned()
    } else {
        let head = git(repository, &["symbolic-ref", "--quiet", "HEAD"]);
        assert!(head.status.success(), "test fixture HEAD must be symbolic");
        String::from_utf8(head.stdout)
            .expect("Git ref is UTF-8")
            .trim()
            .to_owned()
    };
    let remote_branch = branch.replacen("refs/heads/", "refs/remotes/origin/", 1);
    assert!(
        git(repository, &["update-ref", &remote_branch, &branch])
            .status
            .success()
    );
    assert!(
        git(
            repository,
            &["symbolic-ref", "refs/remotes/origin/HEAD", &remote_branch]
        )
        .status
        .success()
    );
}

/// A same-thread reentrant lease on the process environment mutex.
///
/// Nested scoped guards are common when a test combines Git isolation with a
/// `KOOLADE_HOME` override. Other threads still serialize on the mutex, while
/// nested guards on the owning thread share its lease instead of deadlocking.
pub(crate) struct ProcessEnvironmentLock {
    _not_send: PhantomData<MutexGuard<'static, ()>>,
}

impl Drop for ProcessEnvironmentLock {
    fn drop(&mut self) {
        PROCESS_ENVIRONMENT_LOCK_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some((depth, _guard)) = state.as_mut() else {
                return;
            };
            if *depth > 1 {
                *depth -= 1;
            } else {
                *state = None;
            }
        });
    }
}

/// A scoped process-environment override. It serializes only tests that change
/// environment variables and restores their original values during unwinding.
pub(crate) struct ProcessEnvironmentGuard {
    previous: Vec<(OsString, Option<OsString>)>,
    _environment: ProcessEnvironmentLock,
}

impl ProcessEnvironmentGuard {
    pub(crate) fn new() -> Self {
        Self {
            previous: Vec::new(),
            _environment: lock_process_environment(),
        }
    }

    pub(crate) fn set(&mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) {
        let name = name.as_ref();
        self.remember(name);
        // SAFETY: this guard owns the process-wide environment lock until Drop.
        unsafe { std::env::set_var(name, value) };
    }

    pub(crate) fn remove(&mut self, name: impl AsRef<OsStr>) {
        let name = name.as_ref();
        self.remember(name);
        // SAFETY: this guard owns the process-wide environment lock until Drop.
        unsafe { std::env::remove_var(name) };
    }

    fn remember(&mut self, name: &OsStr) {
        if self.previous.iter().any(|(saved, _)| saved == name) {
            return;
        }
        self.previous
            .push((name.to_owned(), std::env::var_os(name)));
    }
}

impl Drop for ProcessEnvironmentGuard {
    fn drop(&mut self) {
        // SAFETY: the environment lock remains held until this guard drops.
        unsafe {
            for (name, value) in self.previous.drain(..).rev() {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }
}

/// Serializes a test's `KOOLADE_HOME` override and restores it even after a panic.
pub(crate) struct KooladeHomeGuard {
    _environment: ProcessEnvironmentGuard,
}

impl KooladeHomeGuard {
    pub(crate) fn set(path: &Path) -> Self {
        let mut environment = ProcessEnvironmentGuard::new();
        environment.set("KOOLADE_HOME", path.as_os_str());
        Self {
            _environment: environment,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessEnvironmentGuard;

    #[test]
    fn environment_removal_is_restored_on_drop() {
        const NAME: &str = "KOOLADE_TEST_ENVIRONMENT_GUARD_RESTORE";
        let previous = std::env::var_os(NAME);
        {
            let mut environment = ProcessEnvironmentGuard::new();
            environment.set(NAME, "temporary");
            environment.remove(NAME);
            assert!(std::env::var_os(NAME).is_none());
        }
        assert_eq!(std::env::var_os(NAME), previous);
    }

    #[test]
    fn nested_environment_guards_share_the_same_thread_lease() {
        let _outer = ProcessEnvironmentGuard::new();
        let _inner = ProcessEnvironmentGuard::new();
    }
}
