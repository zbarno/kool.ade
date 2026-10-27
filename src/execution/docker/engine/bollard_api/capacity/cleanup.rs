use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bollard::{
    models::ContainerSummary,
    query_parameters::{ListContainersOptionsBuilder, RemoveContainerOptionsBuilder},
};

use super::super::BollardApi;
use crate::execution::docker::{discovery::PrerequisiteFailure, images::SelectedImage};

const PROBE_LABEL: &str = "koolade.vm-capacity-probe";
const MAX_RECOVERED_PROBES: usize = 16;
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const MINIMUM_ORPHAN_AGE_SECONDS: i64 = 60;

pub(super) async fn recover_orphaned_probes(
    api: &BollardApi,
    image: &SelectedImage,
    expected_engine_fingerprint: &str,
) -> Result<(), PrerequisiteFailure> {
    tokio::time::timeout(RECOVERY_TIMEOUT, async {
        api.require_approved_engine(image, expected_engine_fingerprint)
            .await?;
        let filters = HashMap::from([("label".to_owned(), vec![PROBE_LABEL.to_owned()])]);
        let options = ListContainersOptionsBuilder::default()
            .all(true)
            .limit((MAX_RECOVERED_PROBES + 1) as i32)
            .filters(&filters)
            .build();
        let containers = api
            .docker
            .list_containers(Some(options))
            .await
            .map_err(|error| super::super::map_api_error(&error))?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| super::cleanup_failure())?
            .as_secs()
            .min(i64::MAX as u64) as i64;
        for name in recovery_batch(containers, now) {
            api.require_approved_engine(image, expected_engine_fingerprint)
                .await?;
            remove_container(&api.docker, &name).await?;
        }
        Ok(())
    })
    .await
    .unwrap_or_else(|_| Err(super::cleanup_failure()))
}

pub(super) async fn cleanup_created_probe(
    api: &BollardApi,
    image: &SelectedImage,
    expected_engine_fingerprint: &str,
    container_id: Option<&str>,
    container_name: &str,
) -> Result<(), PrerequisiteFailure> {
    tokio::time::timeout(Duration::from_secs(5), async {
        api.require_approved_engine(image, expected_engine_fingerprint)
            .await?;
        let target = if let Some(id) = container_id.filter(|id| !id.is_empty()) {
            Some(id.to_owned())
        } else {
            let inspected = match api.docker.inspect_container(container_name, None).await {
                Ok(inspected) => inspected,
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => return Ok(()),
                Err(error) => return Err(super::super::map_api_error(&error)),
            };
            let labels = inspected.config.and_then(|config| config.labels);
            if !inspect_labels_match(container_name, labels.as_ref()) {
                return Err(super::cleanup_failure());
            }
            inspected.id.filter(|id| !id.is_empty())
        };
        let Some(target) = target else {
            return Err(super::cleanup_failure());
        };
        api.require_approved_engine(image, expected_engine_fingerprint)
            .await?;
        remove_container(&api.docker, &target).await
    })
    .await
    .unwrap_or_else(|_| Err(super::cleanup_failure()))
}

async fn remove_container(
    docker: &bollard::Docker,
    target: &str,
) -> Result<(), PrerequisiteFailure> {
    match docker
        .remove_container(
            target,
            Some(
                RemoveContainerOptionsBuilder::default()
                    .force(true)
                    .v(false)
                    .build(),
            ),
        )
        .await
    {
        Ok(())
        | Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        Err(_) => Err(super::cleanup_failure()),
    }
}

fn owned_probe_name(container: &ContainerSummary, now: i64) -> Option<String> {
    let label_name = container.labels.as_ref()?.get(PROBE_LABEL)?;
    if container
        .created
        .is_none_or(|created| now.saturating_sub(created) < MINIMUM_ORPHAN_AGE_SECONDS)
        || !valid_probe_name(label_name)
        || !container
            .names
            .as_ref()?
            .iter()
            .any(|name| name == &format!("/{label_name}"))
    {
        return None;
    }
    Some(label_name.clone())
}

fn recovery_batch(containers: Vec<ContainerSummary>, now: i64) -> Vec<String> {
    containers
        .iter()
        .filter_map(|container| owned_probe_name(container, now))
        .take(MAX_RECOVERED_PROBES)
        .collect()
}

fn valid_probe_name(name: &str) -> bool {
    let id = name.strip_prefix("koolade-capacity-").unwrap_or_default();
    id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn inspect_labels_match(name: &str, labels: Option<&HashMap<String, String>>) -> bool {
    valid_probe_name(name)
        && labels
            .and_then(|labels| labels.get(PROBE_LABEL))
            .is_some_and(|label| label == name)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bollard::models::ContainerSummary;

    use super::{
        MAX_RECOVERED_PROBES, MINIMUM_ORPHAN_AGE_SECONDS, PROBE_LABEL, inspect_labels_match,
        owned_probe_name, recovery_batch, valid_probe_name,
    };

    #[test]
    fn cleanup_targets_require_an_app_owned_random_probe_name_and_label() {
        let name = "koolade-capacity-0123456789abcdef0123456789abcdef";
        let labels = HashMap::from([(PROBE_LABEL.to_owned(), name.to_owned())]);
        assert!(valid_probe_name(name));
        assert!(inspect_labels_match(name, Some(&labels)));
        assert!(!inspect_labels_match(
            "koolade-capacity-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Some(&labels)
        ));
        assert!(!valid_probe_name("koolade-capacity-not-a-uuid"));
    }

    #[test]
    fn startup_recovery_only_selects_old_containers_with_matching_app_labels() {
        let name = "koolade-capacity-0123456789abcdef0123456789abcdef";
        let mut container = ContainerSummary {
            created: Some(100),
            names: Some(vec![format!("/{name}")]),
            labels: Some(HashMap::from([(PROBE_LABEL.to_owned(), name.to_owned())])),
            ..Default::default()
        };
        let now = 100 + MINIMUM_ORPHAN_AGE_SECONDS;
        assert_eq!(owned_probe_name(&container, now).as_deref(), Some(name));
        assert!(owned_probe_name(&container, now - 1).is_none());

        container.names = Some(vec!["/unrelated-container".into()]);
        assert!(owned_probe_name(&container, now).is_none());
    }

    #[test]
    fn orphan_recovery_batches_progress_when_more_than_the_bound_remain() {
        let now = 100 + MINIMUM_ORPHAN_AGE_SECONDS;
        let containers = (0..=MAX_RECOVERED_PROBES)
            .map(|index| {
                let name = format!("koolade-capacity-{index:032x}");
                ContainerSummary {
                    created: Some(100),
                    names: Some(vec![format!("/{name}")]),
                    labels: Some(HashMap::from([(PROBE_LABEL.to_owned(), name)])),
                    ..Default::default()
                }
            })
            .collect::<Vec<_>>();
        let first = recovery_batch(containers.clone(), now);
        assert_eq!(first.len(), MAX_RECOVERED_PROBES);
        let remaining = containers
            .into_iter()
            .filter(|container| {
                owned_probe_name(container, now).is_some_and(|name| !first.contains(&name))
            })
            .collect::<Vec<_>>();
        assert_eq!(recovery_batch(remaining, now).len(), 1);
    }
}
