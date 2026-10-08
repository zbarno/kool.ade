use crate::harness::{DependencyAuthorizationScope, DependencyDecision, DependencyNeed};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, mpsc},
};

#[derive(Debug, Clone)]
pub(crate) struct DependencyResolution {
    pub decision: DependencyDecision,
    pub scope: Option<DependencyAuthorizationScope>,
    pub rationale: String,
}

static PENDING: OnceLock<Mutex<HashMap<String, mpsc::Sender<DependencyResolution>>>> =
    OnceLock::new();
static ONCE_GRANTS: OnceLock<Mutex<Vec<OnceGrant>>> = OnceLock::new();

struct OnceGrant {
    project_id: String,
    task_id: String,
    need: DependencyNeed,
}

fn pending() -> &'static Mutex<HashMap<String, mpsc::Sender<DependencyResolution>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

fn once_grants() -> &'static Mutex<Vec<OnceGrant>> {
    ONCE_GRANTS.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn register(id: &str) -> anyhow::Result<mpsc::Receiver<DependencyResolution>> {
    let (sender, receiver) = mpsc::channel();
    let mut pending = pending()
        .lock()
        .map_err(|_| anyhow::anyhow!("Dependency authorization registry is unavailable"))?;
    anyhow::ensure!(
        !pending.contains_key(id),
        "Dependency authorization request ID is already active"
    );
    pending.insert(id.to_owned(), sender);
    Ok(receiver)
}

pub(crate) fn answer(id: &str, answer: DependencyResolution) -> bool {
    let sender = pending()
        .lock()
        .ok()
        .and_then(|pending| pending.get(id).cloned());
    sender.is_some_and(|sender| sender.send(answer).is_ok())
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
mod tests {
    use super::{DependencyNeed, remember_once_for_request, take_once};
    use crate::harness::{
        DependencyDecision, DependencyFailureCategory, DependencyKind, DependencyRequest,
        DependencyRequestStatus, PackageEcosystem,
    };

    #[test]
    fn one_time_grant_is_project_and_task_scoped_and_consumed_by_one_request() {
        let need = DependencyNeed {
            ecosystem: PackageEcosystem::Npm,
            package: Some("zod".into()),
            version: Some("4.0.0".into()),
            source: Some("https://registry.npmjs.org".into()),
            command: "npm install zod@4.0.0".into(),
            reason: "Validate imported settings data".into(),
            kind: DependencyKind::NewProjectDependency,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        };
        let request = DependencyRequest {
            id: "request-1".into(),
            task_id: "stable-task-uid".into(),
            need: need.clone(),
            category: DependencyFailureCategory::Unknown,
            decision: DependencyDecision::RequiresUserAuthorization,
            rationale: "User decision required".into(),
            risk: "External package source".into(),
            status: DependencyRequestStatus::AwaitingUser,
            preparation: None,
        };
        assert!(remember_once_for_request("project-a", &request));
        assert!(!take_once("project-b", "stable-task-uid", &need));
        assert!(!take_once("project-a", "ticket/path.md", &need));
        let mut different_command = need.clone();
        different_command.command = "npm install zod@4.0.0 --save-prod".into();
        assert!(!take_once(
            "project-a",
            "stable-task-uid",
            &different_command
        ));
        assert!(take_once("project-a", "stable-task-uid", &need));
        assert!(!take_once("project-a", "stable-task-uid", &need));
    }
}
