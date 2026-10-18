use sha2::Digest;

use crate::{
    app::session::Project,
    harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin},
};

mod broker;
use broker::dependency_request_evidence;

/// Application-generated, workspace-private evidence for the generic support
/// check and recent typed lifecycle events. Model output can cite these IDs,
/// but cannot create or alter their contents.
pub(in crate::app::manager::catalog) fn trusted_opportunity_evidence(
    project: &Project,
) -> Vec<EvidenceItem> {
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let mut items = vec![EvidenceItem {
        id: "generic-capability-inventory".into(),
        workspace_uid: workspace_uid.clone(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: "validated project capability inventory".into(),
        content: capability_inventory(project),
    }];
    items.extend(dependency_request_evidence(project, &workspace_uid));
    let feature_uids = project
        .state
        .active_features
        .iter()
        .filter_map(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .map(|identity| identity.uid)
        })
        .collect::<std::collections::BTreeSet<_>>();
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    let mut events = feature_uids
        .iter()
        .flat_map(|uid| lifecycle.events_for_feature(uid).unwrap_or_default())
        .collect::<Vec<_>>();
    events.sort_by_key(|event| (event.created_at_ms, event.uid.clone()));
    for event in events
        .into_iter()
        .rev()
        .take(32)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let event_id = format!("lifecycle-event:{}", event.uid);
        let content = match typed_event_content(project, &event) {
            Some(content) => content,
            None => continue,
        };
        items.push(EvidenceItem {
            id: event_id,
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: "validated typed lifecycle event".into(),
            content,
        });
    }
    for (task, failure) in project.queue.blocked.iter().take(16) {
        let task_uid = project
            .task_documents
            .iter()
            .find(|document| &document.path == task)
            .and_then(|document| document.identity.as_ref())
            .map(|identity| identity.uid.as_str())
            .unwrap_or("unlinked-task");
        let digest = format!(
            "{:x}",
            sha2::Sha256::digest(format!("{workspace_uid}\0{task_uid}").as_bytes())
        );
        items.push(EvidenceItem {
            id: format!("implementation-failure:{}", &digest[..24]),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: "typed implementation queue failure".into(),
            content: format!(
                "task_uid={task_uid} failure_kind={:?} recovery={:?} observed_failure={}",
                failure.kind,
                failure.recovery,
                crate::core::context_build::clip(&failure.message, 1200)
            ),
        });
    }
    items
}

/// Public, compiled product documentation can support a product proposal while
/// the active workspace is a separate repository. It contains no workspace data.
pub(in crate::app::manager::catalog) fn koolade_product_contract_evidence(
    workspace_uid: &str,
) -> EvidenceItem {
    EvidenceItem {
        id: "koolade-product-opportunity-contract".into(),
        workspace_uid: workspace_uid.to_owned(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::Public,
        source: "compiled public Kool.ad/e opportunity-discovery contract".into(),
        content: crate::core::context_build::clip(
            include_str!("../../../../../docs/opportunity-discovery.md"),
            12_000,
        ),
    }
}

fn capability_inventory(project: &Project) -> String {
    let store = crate::project_runtime::ProjectRuntimeStore::new(&project.state.planning_store);
    match store.load_manifest() {
        Ok(Some(manifest)) => {
            let targets = manifest
                .candidate_targets
                .iter()
                .take(32)
                .map(|target| {
                    format!(
                        "target={} surface={:?} state={:?} build={:?} tests={:?} start={:?} dependencies={:?}",
                        target.target_id,
                        target.app_surface,
                        target.capability_state,
                        target.build_recipe_ref,
                        target.test_recipe_refs,
                        target.start_recipe_ref,
                        target.required_dependencies
                    )
                })
                .collect::<Vec<_>>();
            let recipes = manifest
                .recipes
                .iter()
                .chain(&manifest.user_overrides)
                .take(64)
                .map(|recipe| {
                    format!(
                    "recipe={} executable={} source={:?} evidence_refs={:?}",
                        recipe.recipe_id,
                    recipe.executable_id,
                        recipe.source,
                    recipe.evidence_refs
                    )
                })
                .collect::<Vec<_>>();
            crate::core::context_build::clip(&format!(
                "Validated manifest revision {} for repository {} at {}. Languages: {:?}. Frameworks: {:?}. Targets: [{}]. Recipes: [{}]. Treat listed generic recipes/capabilities as available only for their declared target and operation; missing manifest entries are not proof that no other general tool exists.",
                manifest.revision,
                manifest.repository_id,
                manifest.source_sha,
                manifest.detected_languages,
                manifest.frameworks,
                targets.join("; "),
                recipes.join("; ")
            ), 10_000)
        }
        Ok(None) => "No validated project runtime capability manifest is currently persisted. Do not infer that generic CLI, provider, or repository capabilities are absent from this fact alone; inspect the supplied workspace summary and existing recipes before proposing a missing capability.".into(),
        Err(error) => format!("Project runtime capability inventory could not be validated: {}. Treat capability state as unknown and do not claim a capability is missing based on this inventory.", crate::core::context_build::clip(&error.to_string(), 500)),
    }
}

fn typed_event_content(
    project: &Project,
    event: &crate::core::lifecycle::LifecycleEventRecord,
) -> Option<String> {
    use crate::core::lifecycle::{FeatureEvent, LifecycleEventKind, TaskEvent};
    let summary = match &event.event {
        LifecycleEventKind::Feature(FeatureEvent::PlanReview(receipt)) => format!(
            "feature plan review: decision={:?} subject_sha256={} actor={:?}",
            receipt.decision, receipt.subject_sha256, receipt.actor
        ),
        LifecycleEventKind::Feature(event) => format!("feature lifecycle transition: {event:?}"),
        LifecycleEventKind::Task(TaskEvent::RuntimeQaCompleted {
            request_id,
            receipt,
            result_sha256,
            evidence_manifest_sha256,
            ..
        }) => {
            let qa_store = crate::runtime_qa::RuntimeQaStore::new(
                project.state.planning_store.root.join("state/runtime-qa"),
            );
            match qa_store.load_run(request_id) {
                Ok((request, _, _, result))
                    if request.qa_run_id == *request_id
                        && request.parent_task_uid
                            == event.task_uid.as_deref().unwrap_or_default()
                        && format!(
                            "{:x}",
                            sha2::Sha256::digest(serde_json::to_vec(&result).ok()?)
                        ) == *result_sha256 =>
                {
                    let findings = result
                        .findings
                        .iter()
                        .take(32)
                        .map(|finding| {
                            format!(
                                "finding={} severity={:?} requirement={} expected={} observed={} steps={}",
                                finding.finding_uid,
                                finding.severity,
                                finding.requirement_uids.join(","),
                                crate::core::context_build::clip(&finding.expected, 500),
                                crate::core::context_build::clip(&finding.actual, 500),
                                crate::core::context_build::clip(
                                    &finding.reproducible_steps.join(" -> "),
                                    1000
                                )
                            )
                        })
                        .collect::<Vec<_>>();
                    format!(
                        "accepted typed runtime QA report: outcome={:?} receipt={:?} result_sha256={} evidence_sha256={} findings=[{}]",
                        result.outcome,
                        receipt.decision,
                        result_sha256,
                        evidence_manifest_sha256,
                        findings.join("; ")
                    )
                }
                _ => return None,
            }
        }
        LifecycleEventKind::Task(TaskEvent::CodeReview { receipt, .. }) => format!(
            "independent code review: decision={:?} subject_sha256={} actor={:?}",
            receipt.decision, receipt.subject_sha256, receipt.actor
        ),
        LifecycleEventKind::Task(TaskEvent::ReconcileSpec { receipt, .. })
        | LifecycleEventKind::Task(TaskEvent::SubtaskReconciled { receipt }) => format!(
            "specification reconciliation receipt: decision={:?} subject_sha256={} actor={:?}",
            receipt.decision, receipt.subject_sha256, receipt.actor
        ),
        LifecycleEventKind::PullRequestState {
            work_item_uid,
            status,
        } => format!("pull request work item {work_item_uid} state: {status:?}"),
        LifecycleEventKind::Task(event) => format!("task lifecycle transition: {event:?}"),
        LifecycleEventKind::FeatureCreated => "feature created".into(),
        LifecycleEventKind::TaskCreated => "task created".into(),
        LifecycleEventKind::SubtaskCreated { parent_task_uid } => {
            format!("subtask created under parent {parent_task_uid}")
        }
    };
    Some(format!(
        "event_uid={} feature_uid={} task_uid={:?} actor={:?} created_at_ms={} revision={} typed_event={summary}",
        event.uid,
        event.feature_uid,
        event.task_uid,
        event.actor,
        event.created_at_ms,
        event.revision
    ))
}
