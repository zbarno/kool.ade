use crate::app::session::Project;

#[cfg(test)]
#[path = "initial_publication/tests.rs"]
mod tests;

/// Publish the reviewed starter documents through the same isolated PR
/// publisher used for later approved documentation changes.
pub(crate) fn publish_initial_product_specification(
    project: &mut Project,
) -> Result<String, String> {
    let store = &project.state.planning_store;
    let pin = project
        .state
        .published_spec_pin
        .as_ref()
        .ok_or_else(|| "The code repository has no pinned published Git ref.".to_owned())?;
    let reader = crate::core::published_specs::PublishedSpecReader::for_primary_repository(
        &project.state.repo_root,
    )
    .map_err(|error| error.to_string())?;
    reader
        .ensure_local_ref_unchanged(pin)
        .map_err(|error| error.to_string())?;
    let published = reader
        .read_documentation(pin)
        .map_err(|error| error.to_string())?;
    if published
        .iter()
        .any(|document| document.path.starts_with("docs/koolade/"))
    {
        return Err(
            "Existing docs/koolade documentation was found. Review and propose changes to those documents instead of initializing a competing layout.".into(),
        );
    }

    let changes = crate::core::spec_publication::initial_product_documents(&project.state.title);
    ensure_initial_destinations_are_unoccupied(&reader, pin, &changes)
        .map_err(|error| error.to_string())?;
    crate::core::spec_publication::validate_changes(&changes).map_err(|error| error.to_string())?;
    let consent = crate::core::spec_publication::PublicationConsent::confirmed(&changes)
        .map_err(|error| error.to_string())?;
    let receipt_path = format!("state/spec_publications/initial-{}.json", consent.digest());
    match store.read(&receipt_path) {
        Ok(bytes) => {
            let receipt: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            if let Some(url) = receipt
                .get("pullRequestUrl")
                .and_then(|value| value.as_str())
            {
                return Ok(url.to_owned());
            }
            return Err("The saved initial specification publication receipt is invalid.".into());
        }
        Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let pull_request = crate::core::spec_publication::publish_approved(
        &project.state.repo_root,
        &pin.repository_id,
        pin,
        &changes,
        &consent,
    )
    .map_err(|error| {
        format!(
            "The starter documents remain local to the publication attempt; no source checkout was changed: {error}"
        )
    })?;
    let receipt = serde_json::json!({
        "schemaVersion": 1,
        "pullRequestUrl": pull_request.url,
        "branch": pull_request.branch,
        "baseCommit": pull_request.base_commit,
        "headCommit": pull_request.head_commit,
        "documentDigest": consent.digest(),
    });
    store
        .atomic_write(
            &receipt_path,
            &serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
        )
        .map_err(|error| {
            format!(
                "The documentation PR was opened, but its local receipt could not be saved: {error}"
            )
        })?;
    let url = receipt["pullRequestUrl"]
        .as_str()
        .ok_or_else(|| "The publication provider returned an invalid PR URL.".to_owned())?
        .to_owned();
    project.activity.pending.push(format!(
        "Initial product specification PR opened: {url}. Published truth updates after merge and refresh."
    ));
    Ok(url)
}

fn ensure_initial_destinations_are_unoccupied(
    reader: &crate::core::published_specs::PublishedSpecReader,
    pin: &crate::core::published_specs::PublishedSpecPin,
    changes: &[crate::core::spec_publication::DocumentationChange],
) -> anyhow::Result<()> {
    for change in changes {
        if reader.read_optional(pin, &change.path)?.is_some() {
            anyhow::bail!(
                "A starter documentation destination already exists at {}. Review and propose changes instead of overwriting it.",
                change.path
            );
        }
    }
    Ok(())
}
