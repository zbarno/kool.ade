use crate::artifacts::planning_store::PlanningStore;

pub(crate) fn task_repository_id_with_store(
    planning_store: &PlanningStore,
    ticket: &str,
) -> anyhow::Result<String> {
    let (text, _, metadata) = super::read_ticket_and_identity(planning_store, ticket)?;
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_store)?;
    super::task_repository_id(&text, metadata.as_ref(), &manifest)
}
