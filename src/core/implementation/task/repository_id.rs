use crate::artifacts::planning_store::PlanningStore;

pub(crate) fn task_repository_id_with_store(
    planning_store: &PlanningStore,
    ticket: &str,
) -> anyhow::Result<String> {
    let (_, _, metadata) = super::read_ticket_and_identity(planning_store, ticket)?;
    super::task_repository_id(metadata.as_ref())
}
