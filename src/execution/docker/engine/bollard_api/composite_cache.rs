//! Bounded cleanup of app-owned, verified composite runtime images.
//!
//! Docker images can share layers and active containers can retain images.
//! The accounting is deliberately conservative (full image sizes) and cleanup
//! never force-deletes, touches another app's tag, or removes a live image.

use bollard::{
    models::ImageSummary,
    query_parameters::{ListImagesOptions, RemoveImageOptionsBuilder},
};

use super::{BollardApi, failure, map_api_error};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::{CompositionReceipt, is_digest},
};

const MAX_COMPOSITES: usize = 8;
const MAX_COMPOSITE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const COMPOSITION_LABEL: &str = "io.koolade.composition-policy";
const COMPOSITION_POLICY: &str = "project-composition-v1";

#[derive(Clone, Debug, Eq, PartialEq)]
struct CacheEntry {
    tag: String,
    content_identity: String,
    image_id: String,
    estimated_bytes: u64,
    last_used: u64,
    created: i64,
    containers: i64,
}

fn classified_entry(image: &ImageSummary) -> Option<CacheEntry> {
    if image.labels.get(COMPOSITION_LABEL).map(String::as_str)
        != Some(COMPOSITION_POLICY)
    {
        return None;
    }
    let content_identity = image.labels.get("io.koolade.content")?.clone();
    let base_digest = image.labels.get("io.koolade.base-digest")?;
    if !is_digest(&content_identity)
        || !is_digest(base_digest)
        || !is_digest(&image.id)
        || image.size < 0
    {
        return None;
    }
    let tag = format!("koolade-derived:{}", &content_identity[7..]);
    // Refuse to delete aliases, image IDs, shared user tags, and any
    // unrecognized cache objects even when they carry copied labels.
    if image.repo_tags != [tag.clone()] {
        return None;
    }
    let last_used = CompositionReceipt::last_verified_at(&content_identity, &image.id)
        .unwrap_or_else(|| u64::try_from(image.created).unwrap_or_default() * 1000);
    Some(CacheEntry {
        tag,
        content_identity,
        image_id: image.id.clone(),
        estimated_bytes: image.size as u64,
        last_used,
        created: image.created,
        containers: image.containers,
    })
}

fn eviction_plan(
    entries: &[CacheEntry],
    retained_identity: &str,
    max_images: usize,
    max_bytes: u64,
) -> Vec<CacheEntry> {
    let mut count = entries.len();
    let mut used = entries
        .iter()
        .fold(0_u64, |total, entry| total.saturating_add(entry.estimated_bytes));
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| {
        (a.last_used, a.created, &a.tag).cmp(&(b.last_used, b.created, &b.tag))
    });
    let mut evictions = Vec::new();
    for entry in sorted {
        if count <= max_images && used <= max_bytes {
            break;
        }
        if entry.content_identity == retained_identity || entry.containers != 0 {
            continue;
        }
        count -= 1;
        used = used.saturating_sub(entry.estimated_bytes);
        evictions.push(entry);
    }
    evictions
}

pub(super) async fn prune(
    api: &BollardApi,
    retained_identity: &str,
) -> Result<(), PrerequisiteFailure> {
    if !is_digest(retained_identity) {
        return Err(failure(
            PrerequisiteCode::ImageDigestMismatch,
            "Composite cache retention identity is invalid",
            false,
        ));
    }
    let images = api
        .docker
        .list_images(Some(ListImagesOptions {
            all: true,
            ..Default::default()
        }))
        .await
        .map_err(|error| map_api_error(&error))?;
    let candidates = images
        .iter()
        .filter_map(classified_entry)
        .collect::<Vec<_>>();
    for entry in eviction_plan(
        &candidates,
        retained_identity,
        MAX_COMPOSITES,
        MAX_COMPOSITE_BYTES,
    ) {
        // An application-owned tag may have been replaced after list_images.
        // Check the exact image ID and labels again before untagging.
        let inspected = api
            .docker
            .inspect_image(&entry.tag)
            .await
            .map_err(|error| map_api_error(&error))?;
        if inspected.id.as_deref() != Some(entry.image_id.as_str())
            || inspected.config.as_ref().and_then(|config| config.labels.as_ref())
                .and_then(|labels| labels.get(COMPOSITION_LABEL))
                .map(String::as_str) != Some(COMPOSITION_POLICY)
            || inspected.config.as_ref().and_then(|config| config.labels.as_ref())
                .and_then(|labels| labels.get("io.koolade.content"))
                != Some(&entry.content_identity)
        {
            return Err(failure(
                PrerequisiteCode::ImageDigestMismatch,
                "Composite cache changed identity during cleanup",
                false,
            ));
        }
        match api
            .docker
            .remove_image(
                &entry.tag,
                Some(
                    RemoveImageOptionsBuilder::default()
                        .force(false)
                        .noprune(true)
                        .build(),
                ),
                None,
            )
            .await
        {
            Ok(_) => {
                CompositionReceipt::forget_if_matches(
                    &entry.content_identity,
                    &entry.image_id,
                )
                .map_err(|_| {
                    failure(
                        PrerequisiteCode::ImageUnavailable,
                        "Composite cache entry was removed but its receipt could not be cleaned up",
                        false,
                    )
                })?;
            }
            // Docker refuses to delete images still needed by another
            // session. Do not force removal or fail the verified task.
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 409, ..
            }) => {}
            Err(error) => return Err(map_api_error(&error)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn entry(name: char, age: u64, active: bool) -> CacheEntry {
        let digest = format!("sha256:{}", name.to_string().repeat(64));
        CacheEntry {
            tag: format!("koolade-derived:{}", &digest[7..]),
            image_id: digest.clone(),
            content_identity: digest,
            estimated_bytes: 100,
            last_used: age,
            created: age as i64,
            containers: i64::from(active),
        }
    }

    #[test]
    fn only_exact_single_tag_with_composition_labels_is_eligible() {
        let mut summary = ImageSummary::default();
        summary.id = format!("sha256:{}", "a".repeat(64));
        summary.size = 100;
        summary.labels = HashMap::from([
            (COMPOSITION_LABEL.to_owned(), COMPOSITION_POLICY.to_owned()),
            ("io.koolade.content".into(), format!("sha256:{}", "b".repeat(64))),
            ("io.koolade.base-digest".into(), format!("sha256:{}", "c".repeat(64))),
        ]);
        summary.repo_tags = vec![format!("koolade-derived:{}", "b".repeat(64))];
        assert!(classified_entry(&summary).is_some());
        summary.repo_tags.push("another-user:latest".into());
        assert!(classified_entry(&summary).is_none());
        summary.repo_tags.pop();
        summary.labels.remove(COMPOSITION_LABEL);
        assert!(classified_entry(&summary).is_none());
    }

    #[test]
    fn retention_is_lru_bounded_and_never_evicts_in_use_images() {
        let objects = vec![
            entry('a', 1, false),
            entry('b', 2, true),
            entry('c', 3, false),
            entry('d', 4, false),
        ];
        let expected_retained = &objects[3].content_identity;
        let to_evict = eviction_plan(&objects, expected_retained, 2, 200);
        assert_eq!(
            to_evict.iter().map(|item| item.tag.as_str()).collect::<Vec<_>>(),
            vec![objects[0].tag.as_str(), objects[2].tag.as_str()],
        );
        assert!(!to_evict.iter().any(|item| item.containers != 0));
        assert!(!to_evict.iter().any(|item| item.content_identity == *expected_retained));
    }

    #[test]
    fn budget_counts_full_images_conservatively_and_breaks_ties() {
        let candidates = vec![entry('b', 1, false), entry('a', 1, false), entry('c', 2, false)];
        let evicted = eviction_plan(&candidates, &candidates[2].content_identity, 10, 120);
        assert_eq!(evicted.len(), 2);
        assert_eq!(evicted[0].tag, candidates[1].tag);
        assert_eq!(evicted[1].tag, candidates[0].tag);
    }
}
