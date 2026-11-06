use std::path::Path;

use super::super::{
    ClaimStatus, KnowledgeProfileStatus, RepositoryKnowledgeProfileV1, RepositoryRevisionKeyV1,
    ScanOmissionV1, ScanStatsV1, facts,
    navigation::scan_navigation,
    source::{BlobReader, MAX_TOTAL_BYTES, tree},
};
use super::MAX_FACTS;
use super::compute::{
    entry_claims, is_source_or_readme, path_slug, possible_entry_point,
    possible_execution_boundary, readme_claims,
};

pub(super) fn scan_uncached(
    root: &Path,
    key: RepositoryRevisionKeyV1,
    scanned_at_unix_ms: u64,
) -> Result<RepositoryKnowledgeProfileV1, String> {
    let tree = tree(root, &key.commit_sha)?;
    let mut reader = BlobReader::new(root, &key.commit_sha, &tree.files);
    let mut stats = ScanStatsV1 {
        tree_entries_seen: tree.entries_seen.min(u32::MAX as usize) as u32,
        truncated: tree.truncated,
        ..Default::default()
    };
    let mut omissions = tree.omissions;
    let mut manifests = Vec::new();
    let mut build_and_test_conventions = Vec::new();
    let mut dependencies = Vec::new();
    let mut execution_boundaries = Vec::new();
    let mut observed_capabilities = Vec::new();
    let mut entry_points_and_interfaces = Vec::new();

    for path in facts::manifest_paths(&tree.files) {
        match reader.read(&path) {
            Ok((content, reference)) => {
                stats.files_read += 1;
                manifests.push(reference.clone());
                dependencies.extend(
                    facts::dependency_claims(&path, &content, reference.clone())
                        .into_iter()
                        .take(MAX_FACTS.saturating_sub(dependencies.len())),
                );
                if let Some(boundary) = facts::boundary_claim(&path, reference.clone()) {
                    execution_boundaries.push(boundary);
                }
                if matches!(
                    path.rsplit('/').next().unwrap_or_default(),
                    "Cargo.toml" | "package.json" | "pyproject.toml" | "go.mod" | "pom.xml"
                ) {
                    build_and_test_conventions.push(facts::make_claim(
                        format!("build-manifest:{}", path_slug(&path)),
                        "build and test conventions",
                        format!(
                            "{} declares the repository's build or dependency configuration.",
                            path
                        ),
                        reference,
                        ClaimStatus::SupportedBySource,
                        90,
                        Some(1),
                        Some(path.clone()),
                    ));
                }
            }
            Err(reason) => omissions.push(ScanOmissionV1 { path, reason }),
        }
        if manifests.len() >= MAX_FACTS {
            stats.truncated = true;
            break;
        }
    }

    for path in facts::test_paths(&tree.files).into_iter().take(32) {
        match reader.read(&path) {
            Ok((_content, reference)) => {
                stats.files_read += 1;
                build_and_test_conventions.push(facts::make_claim(
                    format!("test-file:{}", path_slug(&path)),
                    "build and test conventions",
                    format!("Automated test source is present at {path}."),
                    reference,
                    ClaimStatus::SupportedBySource,
                    95,
                    Some(1),
                    Some(path.clone()),
                ));
            }
            Err(reason) => omissions.push(ScanOmissionV1 { path, reason }),
        }
        if build_and_test_conventions.len() >= MAX_FACTS {
            stats.truncated = true;
            break;
        }
    }

    let candidate_paths = tree
        .files
        .keys()
        .filter(|path| is_source_or_readme(path))
        .filter(|path| {
            possible_entry_point(path)
                || possible_execution_boundary(path)
                || path
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("README.md"))
        })
        .take(64)
        .cloned()
        .collect::<Vec<_>>();
    for path in candidate_paths {
        let is_readme = path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("README.md"));
        if !possible_entry_point(&path) && !possible_execution_boundary(&path) && !is_readme {
            continue;
        }
        match reader.read(&path) {
            Ok((content, reference)) => {
                stats.files_read += 1;
                entry_points_and_interfaces.extend(
                    entry_claims(&path, &content, reference.clone())
                        .into_iter()
                        .take(MAX_FACTS.saturating_sub(entry_points_and_interfaces.len())),
                );
                if let Some(boundary) = facts::boundary_claim(&path, reference.clone()) {
                    execution_boundaries.push(boundary);
                }
                if is_readme {
                    observed_capabilities.extend(readme_claims(&path, &content, reference));
                }
            }
            Err(reason) => omissions.push(ScanOmissionV1 { path, reason }),
        }
        if entry_points_and_interfaces.len() >= MAX_FACTS {
            stats.truncated = true;
            break;
        }
    }

    let mut top_level_subsystems = Vec::new();
    for (directory, reference_path) in facts::subsystem_paths(&tree.top_directories) {
        let Some(file) = tree.files.get(&reference_path) else {
            continue;
        };
        match reader.read(&reference_path) {
            Ok((_content, reference)) => {
                stats.files_read += 1;
                top_level_subsystems.push(facts::make_claim(
                    format!("subsystem:{}", path_slug(&directory)),
                    "top-level subsystems",
                    format!("The {directory}/ subsystem contains tracked source files."),
                    reference,
                    ClaimStatus::Inferred,
                    65,
                    Some(1),
                    Some(file.path.clone()),
                ));
            }
            Err(reason) => omissions.push(ScanOmissionV1 {
                path: reference_path,
                reason,
            }),
        }
    }

    let navigation_roots = [
        "AGENTS.md",
        "docs/AGENTS.md",
        "doc/AGENTS.md",
        "documentation/AGENTS.md",
        "wiki/AGENTS.md",
    ];
    let mut navigation_candidates = Vec::new();
    for root_index in navigation_roots
        .iter()
        .filter(|path| tree.files.contains_key(**path))
    {
        let navigation = scan_navigation(root_index, &key.commit_sha, |path| {
            let (content, reference) = reader.read(path)?;
            stats.files_read = stats.files_read.saturating_add(1);
            Ok((content, reference.content_sha256))
        });
        stats.navigation_indexes_read = stats
            .navigation_indexes_read
            .saturating_add(navigation.indexes_read);
        stats.documents_read = stats
            .documents_read
            .saturating_add(navigation.documents_read);
        stats.truncated |= navigation.truncated;
        navigation_candidates.extend(navigation.candidates);
        omissions.extend(navigation.omissions);
        if navigation_candidates.len() >= 128 {
            navigation_candidates.truncate(128);
            stats.truncated = true;
            break;
        }
    }
    stats.bytes_read = reader.bytes_read().min(MAX_TOTAL_BYTES) as u64;
    let mut remaining_items = MAX_FACTS;
    let mut truncated_items = false;
    truncate_items(&mut manifests, &mut remaining_items, &mut truncated_items);
    truncate_items(
        &mut build_and_test_conventions,
        &mut remaining_items,
        &mut truncated_items,
    );
    truncate_items(
        &mut top_level_subsystems,
        &mut remaining_items,
        &mut truncated_items,
    );
    truncate_items(
        &mut entry_points_and_interfaces,
        &mut remaining_items,
        &mut truncated_items,
    );
    truncate_items(
        &mut dependencies,
        &mut remaining_items,
        &mut truncated_items,
    );
    truncate_items(
        &mut execution_boundaries,
        &mut remaining_items,
        &mut truncated_items,
    );
    truncate_items(
        &mut observed_capabilities,
        &mut remaining_items,
        &mut truncated_items,
    );
    truncate_items(
        &mut navigation_candidates,
        &mut remaining_items,
        &mut truncated_items,
    );
    stats.truncated |= truncated_items;
    if omissions.len() > 128 {
        omissions.truncate(128);
        stats.truncated = true;
    }
    let status = if tree.truncated || stats.truncated || !omissions.is_empty() || key.dirty_checkout
    {
        KnowledgeProfileStatus::Partial
    } else {
        KnowledgeProfileStatus::Complete
    };
    let profile = RepositoryKnowledgeProfileV1 {
        schema_version: 1,
        key,
        scanned_at_unix_ms,
        status,
        languages: facts::languages(&tree.files),
        manifests,
        build_and_test_conventions,
        top_level_subsystems,
        entry_points_and_interfaces,
        dependencies,
        execution_boundaries,
        observed_capabilities,
        navigation_candidates,
        omissions,
        scan: stats,
    };
    profile.validate()?;
    Ok(profile)
}

fn truncate_items<T>(items: &mut Vec<T>, remaining: &mut usize, truncated: &mut bool) {
    if items.len() > *remaining {
        items.truncate(*remaining);
        *truncated = true;
    }
    *remaining = remaining.saturating_sub(items.len());
}
