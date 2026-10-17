use super::{KooladeApp, Screen};
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt, LifecycleStore, TaskPhase};

impl KooladeApp {
    pub(super) fn generate_runtime_qa_scenarios(&mut self, ticket: &str) {
        let prepared = match &self.screen {
            Screen::Connected(project) => request_for_ticket(project, ticket),
            Screen::Welcome => {
                Err("Connect a project before generating runtime QA scenarios.".into())
            }
        };
        let (request, task_revision, workspace_uid, repo_root, store_root) = match prepared {
            Ok(value) => value,
            Err(error) => {
                self.toasts.warning(error);
                return;
            }
        };
        if let Err(error) = crate::app::root::tick::activity::runtime_qa::validate_request_fresh(
            match &self.screen {
                Screen::Connected(project) => project,
                Screen::Welcome => return,
            },
            &request.parent_task_uid,
            &request,
        ) {
            self.toasts.warning(error);
            return;
        }
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.activity.runtime_qa_scenario_synthesis.is_some() {
            self.toasts
                .warning("A runtime QA scenario proposal is already being generated.");
            return;
        }
        if crate::runtime_qa::RuntimeQaStore::new(&store_root)
            .load_scenario_proposal(&request)
            .is_ok()
        {
            self.toasts
                .warning("A scenario proposal already exists for this immutable QA request.");
            return;
        }
        let harness = crate::app::root::configured_harness_for_task(
            &mut self.task_harness,
            Some(crate::persistence::harness_settings::QA),
            &Default::default(),
        );
        project.activity.runtime_qa_scenario_synthesis = Some(
            crate::runtime_qa::scenario_synthesis::ScenarioSynthesisHandle::start(
                request,
                task_revision,
                workspace_uid,
                repo_root,
                harness,
            ),
        );
        self.toasts.success("Generating a runtime QA scenario proposal from the approved criteria and finished build evidence.");
    }

    pub(super) fn approve_runtime_qa_scenarios(
        &mut self,
        ticket: &str,
        expected_revision: u64,
        expected_proposal_sha256: &str,
    ) {
        let result = match &self.screen {
            Screen::Connected(project) => {
                approve(project, ticket, expected_revision, expected_proposal_sha256)
            }
            Screen::Welcome => {
                Err("Connect a project before approving runtime QA scenarios.".into())
            }
        };
        match result {
            Ok(()) => self.toasts.success("Approved the exact runtime QA scenario proposal. The dispatcher will revalidate it before any QA run."),
            Err(error) => self.toasts.warning(error),
        }
    }
}

fn request_for_ticket(
    project: &crate::app::session::Project,
    ticket: &str,
) -> Result<
    (
        crate::runtime_qa::QaRunRequestV1,
        u64,
        String,
        std::path::PathBuf,
        std::path::PathBuf,
    ),
    String,
> {
    let document = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .ok_or_else(|| "Task details no longer identify a saved task.".to_owned())?;
    let item_uid = document
        .identity
        .as_ref()
        .map(|identity| identity.uid.as_str())
        .ok_or_else(|| "Task has no stable planning identity.".to_owned())?;
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle
        .task_for_planning_item(item_uid)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task lifecycle record is unavailable.".to_owned())?;
    if task.phase != TaskPhase::AwaitingRuntimeQa {
        return Err(
            "Runtime QA scenarios can only be generated for a task awaiting runtime QA.".into(),
        );
    }
    let request = task
        .qa_execution
        .as_ref()
        .and_then(|state| state.request.clone())
        .ok_or_else(|| "The immutable reviewed build request is not ready.".to_owned())?;
    request.validate()?;
    let store_root = crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
        project.state.planning_store.project_id,
    )?;
    Ok((
        request,
        task.revision,
        project
            .state
            .planning_store
            .project_id
            .hyphenated()
            .to_string(),
        project.state.repo_root.clone(),
        store_root,
    ))
}

fn approve(
    project: &crate::app::session::Project,
    ticket: &str,
    expected_revision: u64,
    expected_proposal_sha256: &str,
) -> Result<(), String> {
    let (request, task_revision, _, _, store_root) = request_for_ticket(project, ticket)?;
    crate::app::root::tick::activity::runtime_qa::validate_request_fresh(
        project,
        &request.parent_task_uid,
        &request,
    )?;
    if task_revision != expected_revision {
        return Err("The task changed since these scenarios were shown; reload the proposal before approving.".into());
    }
    let store = crate::runtime_qa::RuntimeQaStore::new(store_root);
    let proposal = store.load_scenario_proposal(&request)?;
    let proposal_sha256 = proposal.proposal_sha256()?;
    if proposal_sha256 != expected_proposal_sha256 || proposal.task_revision != expected_revision {
        return Err("This scenario proposal is stale or its exact hash does not match the displayed proposal.".into());
    }
    if store
        .load_approved_scenario_proposal(&request, expected_revision)
        .is_ok()
    {
        return Ok(());
    }
    let receipt_uid = uuid::Uuid::new_v5(
        &uuid::Uuid::parse_str(&request.parent_task_uid).map_err(|error| error.to_string())?,
        format!("runtime-qa-scenario-approval:v1:{expected_revision}:{proposal_sha256}").as_bytes(),
    )
    .hyphenated()
    .to_string();
    let approval = GateReceipt {
        receipt_uid,
        subject_uid: request.parent_task_uid.clone(),
        subject_sha256: proposal.scenario_sha256.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: now_ms(),
        evidence_refs: vec![
            format!("runtime-qa:{}/approved-scenarios", request.qa_run_id),
            format!(
                "runtime-qa:{}/proposal:{proposal_sha256}:task-revision:{expected_revision}",
                request.qa_run_id
            ),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    approval.validate()?;
    store.approve_scenario_proposal(&request, &proposal, expected_revision, &approval)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(1)
}
