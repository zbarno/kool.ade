use crate::harness::{DependencyAuthorizationScope, DependencyDecision, DependencyNeed};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, mpsc},
    time::Duration,
};

const MAX_PENDING_PER_TASK: usize = 8;
const MAX_PENDING_GLOBAL: usize = 64;

#[derive(Debug, Clone)]
pub(crate) struct DependencyResolution {
    pub decision: DependencyDecision,
    pub scope: Option<DependencyAuthorizationScope>,
    pub rationale: String,
}

static PENDING: OnceLock<Mutex<HashMap<String, PendingAuthorization>>> = OnceLock::new();
static ONCE_GRANTS: OnceLock<Mutex<Vec<OnceGrant>>> = OnceLock::new();

struct PendingAuthorization {
    task_id: String,
    need: DependencyNeed,
    sender: mpsc::Sender<DependencyResolution>,
    awaiting_user: bool,
}

pub(crate) struct ResolutionRegistration {
    id: String,
    receiver: mpsc::Receiver<DependencyResolution>,
}

impl ResolutionRegistration {
    pub(crate) fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<DependencyResolution, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }

    pub(crate) fn unregister(&self) {
        unregister(&self.id);
    }
}

impl Drop for ResolutionRegistration {
    fn drop(&mut self) {
        unregister(&self.id);
    }
}

struct OnceGrant {
    project_id: String,
    task_id: String,
    need: DependencyNeed,
}

fn pending() -> &'static Mutex<HashMap<String, PendingAuthorization>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

fn once_grants() -> &'static Mutex<Vec<OnceGrant>> {
    ONCE_GRANTS.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn register(
    id: &str,
    task_id: &str,
    need: &DependencyNeed,
) -> anyhow::Result<ResolutionRegistration> {
    let (sender, receiver) = mpsc::channel();
    let mut pending = pending()
        .lock()
        .map_err(|_| anyhow::anyhow!("Dependency authorization registry is unavailable"))?;
    anyhow::ensure!(
        !pending.contains_key(id),
        "Dependency authorization request ID is already active"
    );
    anyhow::ensure!(
        pending.len() < MAX_PENDING_GLOBAL
            && pending
                .values()
                .filter(|request| request.task_id == task_id)
                .count()
                < MAX_PENDING_PER_TASK,
        "Too many dependency authorizations are already outstanding; resolve or cancel an existing request first"
    );
    pending.insert(
        id.to_owned(),
        PendingAuthorization {
            task_id: task_id.to_owned(),
            need: need.clone(),
            sender,
            awaiting_user: false,
        },
    );
    Ok(ResolutionRegistration {
        id: id.to_owned(),
        receiver,
    })
}

pub(crate) fn answer(
    id: &str,
    task_id: &str,
    need: &DependencyNeed,
    answer: DependencyResolution,
) -> bool {
    let Ok(mut pending) = pending().lock() else {
        return false;
    };
    let Some(request) = pending.get_mut(id) else {
        return false;
    };
    if request.task_id != task_id || request.need != *need {
        return false;
    }
    if answer.decision == DependencyDecision::RequiresUserAuthorization {
        if request.awaiting_user {
            return false;
        }
        request.awaiting_user = true;
        return request.sender.send(answer).is_ok();
    }
    let Some(request) = pending.remove(id) else {
        return false;
    };
    request.sender.send(answer).is_ok()
}

pub(crate) fn unregister(id: &str) {
    if let Ok(mut pending) = pending().lock() {
        pending.remove(id);
    }
}

pub(crate) fn remember_once(project_id: &str, task_id: &str, need: &DependencyNeed) -> bool {
    let Ok(mut grants) = once_grants().lock() else {
        return false;
    };
    grants.retain(|grant| {
        !(grant.project_id == project_id
            && grant.task_id == task_id
            && same_need(&grant.need, need))
    });
    if grants.len() >= 512 {
        grants.remove(0);
    }
    grants.push(OnceGrant {
        project_id: project_id.to_owned(),
        task_id: task_id.to_owned(),
        need: need.clone(),
    });
    true
}

pub(crate) fn remember_once_for_request(
    project_id: &str,
    request: &crate::harness::DependencyRequest,
) -> bool {
    remember_once(project_id, &request.task_id, &request.need)
}

pub(crate) fn take_once(project_id: &str, task_id: &str, need: &DependencyNeed) -> bool {
    let Ok(mut grants) = once_grants().lock() else {
        return false;
    };
    let Some(position) = grants.iter().position(|grant| {
        grant.project_id == project_id && grant.task_id == task_id && same_need(&grant.need, need)
    }) else {
        return false;
    };
    grants.remove(position);
    true
}

fn same_need(left: &DependencyNeed, right: &DependencyNeed) -> bool {
    left.ecosystem == right.ecosystem
        && left.package == right.package
        && left.version == right.version
        && left.source == right.source
        && left.kind == right.kind
        && left.command == right.command
        && left.lockfile_identity == right.lockfile_identity
        && left.introduced_packages == right.introduced_packages
}

#[cfg(test)]
#[path = "dependency_authorization/tests.rs"]
mod tests;
