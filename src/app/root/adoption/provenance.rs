use super::*;
use sha2::Digest;

impl KooladeApp {
    pub(super) fn record_plan_author_provenance(
        &mut self,
        project: &mut Project,
        document_updates: &[(String, String)],
        author_provenance: Option<(String, String)>,
    ) {
        if let Some((author_run_id, author_session_id)) = author_provenance {
            for (document_id, _) in document_updates {
                let Some(feature_id) = document_id.strip_prefix("feature:") else {
                    continue;
                };
                let Some((_, markdown)) = project
                    .state
                    .active_features
                    .iter()
                    .find(|(id, _)| id == feature_id)
                else {
                    continue;
                };
                let Ok(Some(identity)) = crate::domain::ArtifactIdentity::from_markdown(markdown)
                else {
                    continue;
                };
                let contract = crate::core::workflow::feature_contract(markdown);
                let provenance = crate::core::lifecycle::plan_snapshot::PlanAuthorProvenanceV1 {
                    feature_uid: identity.uid,
                    approval_contract_sha256: format!(
                        "{:x}",
                        sha2::Sha256::digest(contract.as_bytes())
                    ),
                    author_run_id: author_run_id.clone(),
                    author_session_id: author_session_id.clone(),
                };
                let store = &project.state.planning_store;
                let _guard = crate::core::writer_gate::acquire();
                let record_result =
                    crate::core::lifecycle::plan_snapshot::record_author(store, &provenance);
                drop(_guard);
                if let Err(error) = record_result {
                    self.toasts.warning(format!(
                        "Plan author provenance for {feature_id} could not be recorded: {error}"
                    ));
                } else {
                    let path = format!(
                        "state/lifecycle_plan_authors/{}.json",
                        provenance.feature_uid
                    );
                    if let Err(error) = crate::core::gitops::commit(
                        &store.git_root(),
                        &format!("planner: record plan author provenance {feature_id}"),
                        &[store.git_path(&path)],
                    ) {
                        self.toasts.warning(format!(
                                "Plan author provenance for {feature_id} was saved but its Git checkpoint failed: {error}"
                            ));
                    }
                }
            }
        }
    }
}
