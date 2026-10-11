use super::{
    ClaimAttempt, ClaimAttemptMode, ClaimError, ClaimLease, ClaimRequest, ClaimScope, git,
};
use std::path::PathBuf;

impl ClaimRequest {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        repo: PathBuf,
        code_repo: PathBuf,
        project_uid: uuid::Uuid,
        task_uid: String,
        code_repository_id: String,
        base_commit: String,
        stale_session: Option<String>,
        scope: ClaimScope,
        allow_offline: bool,
    ) -> Self {
        Self {
            repo,
            code_repo,
            project_uid,
            task_uid,
            code_repository_id,
            base_commit,
            stale_session,
            scope,
            allow_offline,
        }
    }

    pub(crate) fn acquire(self) -> Result<ClaimAttempt, ClaimError> {
        if self.scope == ClaimScope::LocalOnly {
            return Ok(attempt(None, None, ClaimAttemptMode::LocalOnly));
        }
        let project_uid = match resolve_project_uid(&self) {
            Ok(Some(id)) => id,
            Ok(None) => {
                return Ok(attempt(
                    None,
                    Some("No planning remote is configured; this run is protected only by the local queue lock.".into()),
                    ClaimAttemptMode::LocalOnly,
                ));
            }
            Err(ClaimError::RemoteUnavailable(detail)) if self.allow_offline => {
                return Ok(attempt(
                    None,
                    Some(format!(
                        "The shared planning remote claim could not be acquired; proceeding with the local queue lock only: {detail}"
                    )),
                    ClaimAttemptMode::UnsafeOffline,
                ));
            }
            Err(error) => return Err(error),
        };
        let legacy_claim = match find_legacy_claim(&self) {
            Ok(claim) => claim,
            Err(ClaimError::RemoteUnavailable(detail)) if self.allow_offline => {
                return Ok(attempt(
                    None,
                    Some(format!(
                        "The old task claim could not be checked; proceeding with the local queue lock only: {detail}"
                    )),
                    ClaimAttemptMode::UnsafeOffline,
                ));
            }
            Err(error) => return Err(error),
        };
        let workspace_claim_exists = match git::workspace_claim_exists(
            &self.repo,
            project_uid,
            &self.task_uid,
        ) {
            Ok(exists) => exists,
            Err(ClaimError::RemoteUnavailable(detail)) if self.allow_offline => {
                return Ok(attempt(
                    None,
                    Some(format!(
                        "The workspace task claim could not be checked; no legacy claim was changed, and this local-only run cannot publish: {detail}"
                    )),
                    ClaimAttemptMode::UnsafeOffline,
                ));
            }
            Err(error) => return Err(error),
        };
        if workspace_claim_exists && legacy_claim.is_some() {
            return Err(ClaimError::CoordinationRejected(format!(
                "Execution lease conflict for task {}: legacy code-repository claim and workspace planning claim both exist; both refs are preserved. Resolve them explicitly before retrying",
                self.task_uid
            )));
        }
        if legacy_claim.is_some() && git::push_remote(&self.repo)?.is_none() {
            let error = ClaimError::RemoteUnavailable(
                "Cannot retire a legacy task claim without a reachable planning push remote".into(),
            );
            if self.allow_offline {
                return Ok(attempt(
                    None,
                    Some(format!(
                        "The legacy task claim was preserved because the planning remote is unavailable; this local-only run cannot publish: {error}"
                    )),
                    ClaimAttemptMode::UnsafeOffline,
                ));
            }
            return Err(error);
        }
        let legacy_recovery = if let Some(claim) = legacy_claim {
            let timestamp = git::now()?;
            git::validate_clock(&claim.record, timestamp)?;
            let stale = claim.record.appears_stale(timestamp);
            if self.stale_session.as_deref() != Some(claim.record.session_id.as_str()) || !stale {
                return Err(ClaimError::AlreadyClaimed {
                    record: Box::new(claim.record),
                    stale,
                });
            }
            Some(claim)
        } else {
            None
        };
        // Retire a confirmed-stale task-only claim before acquiring the new
        // project-scoped lease. Older clients atomically publish a code branch
        // with their legacy claim ref; deleting that ref first makes their
        // expected-SHA push lose before this process can start work.
        if let Some(legacy) = legacy_recovery.as_ref()
            && let Err(error) = git::remove_expired_legacy_claim(
                &legacy.source_repo,
                legacy,
                self.stale_session.as_deref().unwrap_or_default(),
            )
        {
            if let ClaimError::RemoteUnavailable(detail) = &error
                && self.allow_offline
            {
                return Ok(attempt(
                    None,
                    Some(format!(
                        "The old task claim could not be cleared; proceeding with the local queue lock only: {detail}"
                    )),
                    ClaimAttemptMode::UnsafeOffline,
                ));
            }
            return Err(error);
        }
        let result = if let Some(session) = self
            .stale_session
            .as_deref()
            .filter(|_| legacy_recovery.is_none())
        {
            ClaimLease::take_over_stale(
                &self.repo,
                project_uid,
                &self.task_uid,
                &self.code_repository_id,
                session,
                &self.base_commit,
            )
            .map(Some)
        } else {
            ClaimLease::acquire(
                &self.repo,
                project_uid,
                &self.task_uid,
                &self.code_repository_id,
                &self.base_commit,
            )
        };
        match result {
            Ok(Some(lease)) => Ok(attempt(Some(lease), None, ClaimAttemptMode::Shared)),
            Ok(None) if self.scope == ClaimScope::SharedOptional => Ok(attempt(
                None,
                Some("No planning remote is configured; this run is protected only by the local queue lock.".into()),
                ClaimAttemptMode::LocalOnly,
            )),
            Ok(None) if self.allow_offline => Ok(attempt(
                None,
                Some("No planning remote is configured; proceeding with the local queue lock only. This run cannot publish until shared coordination is restored.".into()),
                ClaimAttemptMode::UnsafeOffline,
            )),
            Ok(None) => Err(ClaimError::RemoteUnavailable(
                "No planning remote is configured for shared task coordination".into(),
            )),
            Err(ClaimError::RemoteUnavailable(detail)) if self.allow_offline => Ok(attempt(
                None,
                Some(format!(
                    "The shared planning remote claim could not be acquired; proceeding with the local queue lock only: {detail}"
                )),
                ClaimAttemptMode::UnsafeOffline,
            )),
            Err(error) => Err(error),
        }
    }
}

fn resolve_project_uid(request: &ClaimRequest) -> Result<Option<uuid::Uuid>, ClaimError> {
    if !request.project_uid.is_nil() {
        return Ok(Some(request.project_uid));
    }
    match (request.scope, git::legacy_project_uid(&request.repo)?) {
        (ClaimScope::SharedOptional, None) => Ok(None),
        (_, None) => Err(ClaimError::RemoteUnavailable(
            "No planning remote is configured for shared task coordination".into(),
        )),
        (ClaimScope::SharedOptional, Some(id)) => Ok(Some(id)),
        (_, Some(_)) => Err(ClaimError::CoordinationRejected(
            "Shared planning claims require a stable project UUID".into(),
        )),
    }
}

fn find_legacy_claim(request: &ClaimRequest) -> Result<Option<git::LegacyClaim>, ClaimError> {
    let mut claims = Vec::new();
    for repo in [&request.repo, &request.code_repo] {
        if let Some(claim) = git::read_legacy_claim(repo, &request.task_uid)?
            && !claims.iter().any(|current: &git::LegacyClaim| {
                current.remote == claim.remote && current.reference == claim.reference
            })
        {
            claims.push(claim);
        }
    }
    if claims.len() > 1 {
        return Err(ClaimError::CoordinationRejected(
            "Multiple prior task-only claims exist on different remotes; resolve the old claims before starting".into(),
        ));
    }
    Ok(claims.pop())
}

fn attempt(
    lease: Option<ClaimLease>,
    warning: Option<String>,
    mode: ClaimAttemptMode,
) -> ClaimAttempt {
    ClaimAttempt {
        lease,
        warning,
        mode,
    }
}
