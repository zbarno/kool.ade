use sha2::Digest;

use crate::{
    app::session::Project,
    harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin},
};

mod events;
use events::typed_event_content;
mod telemetry;

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
    items.extend(telemetry::regression_evidence(project, &workspace_uid));
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
    let specification_findings = lifecycle
        .product_specification_aggregate()
        .map(|aggregate| aggregate.findings)
        .unwrap_or_default();
    items.extend(events::specification_finding_evidence(
        &workspace_uid,
        &specification_findings,
    ));
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
        let mismatch =
            failure.kind == crate::core::implementation::FailureKind::ProviderCapabilityMismatch;
        items.push(EvidenceItem {
            id: format!(
                "{}:{}",
                if mismatch {
                    "provider-capability-mismatch"
                } else {
                    "implementation-failure"
                },
                &digest[..24]
            ),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: if mismatch {
                "typed provider capability mismatch"
            } else {
                "typed implementation queue failure"
            }
            .into(),
            content: if mismatch {
                format!(
                    "task_uid={task_uid} recovery={:?} observed_capability_mismatch={}",
                    failure.recovery,
                    crate::core::context_build::clip(&failure.message, 512)
                )
            } else {
                format!(
                    "task_uid={task_uid} failure_kind={:?} recovery={:?} observed_failure={}",
                    failure.kind,
                    failure.recovery,
                    crate::core::context_build::clip(&failure.message, 1200)
                )
            },
        });
    }
    items
}

/// Designated model-shareable product documentation, compiled into the app.
/// These files must contain only public product information even when edited
/// locally, because uncommitted build-tree contents are included here.
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

pub(in crate::app::manager::catalog) fn koolade_product_capability_evidence(
    workspace_uid: &str,
) -> EvidenceItem {
    EvidenceItem {
        id: "koolade-product-capability-inventory".into(),
        workspace_uid: workspace_uid.to_owned(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::Public,
        source: "designated model-shareable Kool.ad/e capability inventory".into(),
        content: include_str!("../../../../../assets/capabilities/active-capabilities.json").into(),
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
