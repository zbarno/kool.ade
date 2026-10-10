use crate::artifacts::planning_store::PlanningStore;

pub(super) fn apply(
    store: &PlanningStore,
    changes: &[(String, String)],
    expected_revision: &str,
) -> anyhow::Result<(Vec<String>, String)> {
    let encoded = changes
        .iter()
        .map(|(path, text)| {
            let relative = path
                .strip_prefix(".koolade-packet/")
                .unwrap_or(path)
                .to_owned();
            (relative, text.as_bytes().to_vec())
        })
        .collect::<Vec<_>>();
    store
        .transaction_with_revision(&encoded, Some(expected_revision))
        .map(|(paths, revision)| {
            (
                paths.iter().map(|path| store.git_path(path)).collect(),
                revision,
            )
        })
        .map_err(anyhow::Error::new)
}
