use super::*;
use crate::core::published_specs::PublishedSpecPin;
use std::{fs, os::unix::fs::PermissionsExt};

/// Production boundary: both app-connected installations publish through the
/// core spec publication coordinator, which binds each request to its pinned
/// SHA and exact consented docs. The invariant is that a stale second install
/// is rejected, then can publish only after refreshing the baseline and
/// resolving the doc set. Direct Git commands cannot prove this provider
/// sequencing or consent guard. Uses local Git, a bare remote, and fake `gh`;
/// no Docker/network, moderate setup.
pub(super) fn publish_conflicting_updates(
    first: &Path,
    second: &Path,
    repository_id: &str,
    first_pin: &PublishedSpecPin,
    second_pin: &PublishedSpecPin,
    bin: &Path,
) {
    fs::create_dir_all(bin).unwrap();
    let gh = fake_gh(bin);

    let first_original = fs::read_to_string(first.join(SPEC)).unwrap();
    let first_content =
        first_original.replace("Test product context.", "Published from installation one.");
    let first_changes = changes(&first_content);
    let first_pr = crate::app::root::publish_feature_document_set_for_test(
        first,
        repository_id,
        first_pin,
        &first_changes,
        |root, repository_id, pin, changes, consent| {
            crate::core::spec_publication::publish_with_test_gh(
                root,
                repository_id,
                pin,
                changes,
                consent,
                gh.as_os_str(),
            )
        },
    )
    .unwrap();
    assert_eq!(first_pr.base_commit, first_pin.commit_sha);
    merge_pr(first, &first_pr.branch);

    let second_original = fs::read_to_string(second.join(SPEC)).unwrap();
    let stale_content =
        second_original.replace("Test product context.", "Published from installation two.");
    let stale_error = crate::app::root::publish_feature_document_set_for_test(
        second,
        repository_id,
        second_pin,
        &changes(&stale_content),
        |root, repository_id, pin, changes, consent| {
            crate::core::spec_publication::publish_with_test_gh(
                root,
                repository_id,
                pin,
                changes,
                consent,
                gh.as_os_str(),
            )
        },
    )
    .unwrap_err();
    assert!(
        stale_error
            .to_string()
            .contains("published documentation changed on the remote"),
        "stale installation must be rejected before PR creation: {stale_error:#}"
    );
    assert_eq!(
        fs::read_to_string(bin.join("create-count"))
            .unwrap()
            .lines()
            .count(),
        1,
        "stale request must not create a second PR"
    );

    git(second, &["fetch", "--quiet", "origin"]);
    let refreshed_reader = crate::core::published_specs::PublishedSpecReader::new(
        second.to_path_buf(),
        second_pin.repository_id.clone(),
        second_pin.reference.clone(),
    )
    .unwrap();
    let refreshed_pin = refreshed_reader.pin().unwrap();
    assert_ne!(refreshed_pin.commit_sha, second_pin.commit_sha);
    let refreshed = refreshed_reader.read(&refreshed_pin, SPEC).unwrap().content;
    assert!(refreshed.contains("Published from installation one."));
    let resolved_content = format!("{refreshed}\nPublished from installation two.\n");
    let resolved_changes = changes(&resolved_content);
    let second_pr = crate::app::root::publish_feature_document_set_for_test(
        second,
        repository_id,
        &refreshed_pin,
        &resolved_changes,
        |root, repository_id, pin, changes, consent| {
            crate::core::spec_publication::publish_with_test_gh(
                root,
                repository_id,
                pin,
                changes,
                consent,
                gh.as_os_str(),
            )
        },
    )
    .unwrap();
    assert_eq!(second_pr.base_commit, refreshed_pin.commit_sha);
    assert_eq!(
        fs::read_to_string(bin.join("create-count"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    merge_pr(first, &second_pr.branch);
}

fn changes(content: &str) -> Vec<crate::core::spec_publication::DocumentationChange> {
    vec![crate::core::spec_publication::DocumentationChange {
        path: SPEC.into(),
        content: content.into(),
    }]
}

fn merge_pr(repository: &Path, branch: &str) {
    git(repository, &["fetch", "--quiet", "origin", branch]);
    git(repository, &["merge", "--quiet", "--ff-only", "FETCH_HEAD"]);
    git(repository, &["push", "--quiet", "origin", "main"]);
}

fn fake_gh(bin: &Path) -> std::path::PathBuf {
    let gh = bin.join("gh");
    let response = bin.join("pull-request.json");
    let latest_head = bin.join("latest-head");
    let latest_url = bin.join("latest-url");
    let count = bin.join("create-count");
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = repo ]; then echo zbarno/spec-test; exit 0; fi
if [ "$1" = api ]; then printf '987654321\ttest-operator\n'; exit 0; fi
if [ "$1" = pr ] && [ "$2" = view ]; then
  if [ -f '{}' ] && {{ [ "$3" = "$(cat '{}')" ] || [ "$3" = "$(cat '{}')" ]; }}; then cat '{}'; exit 0; fi
  exit 1
fi
if [ "$1" = pr ] && [ "$2" = create ]; then
  base=""; branch=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --base) base="$2"; shift 2 ;;
      --head) branch="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  n=0; [ -f '{}' ] && n=$(wc -l < '{}')
  n=$((n + 1)); echo "$n" >> '{}'
  base_sha=$(git ls-remote origin "refs/heads/$base" | cut -f1)
  head_sha=$(git ls-remote origin "refs/heads/$branch" | cut -f1)
  url="https://github.com/zbarno/spec-test/pull/$n"
  printf '%s\n' "$branch" > '{}'
  printf '%s\n' "$url" > '{}'
  printf '{{"url":"%s","baseRefName":"%s","baseRefOid":"%s","headRefName":"%s","headRefOid":"%s","headRepository":{{"nameWithOwner":"zbarno/spec-test"}}}}\n' "$url" "$base" "$base_sha" "$branch" "$head_sha" > '{}'
  echo "$url"
  exit 0
fi
exit 1
"#,
        latest_head.display(),
        latest_head.display(),
        latest_url.display(),
        response.display(),
        count.display(),
        count.display(),
        count.display(),
        latest_head.display(),
        latest_url.display(),
        response.display(),
    );
    fs::write(&gh, script).unwrap();
    let mut permissions = fs::metadata(&gh).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&gh, permissions).unwrap();
    gh
}
