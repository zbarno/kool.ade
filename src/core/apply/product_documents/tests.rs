use super::*;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::published_specs::PublishedSpecPin;

fn store(name: &str) -> (tempfile::TempDir, PlanningStore) {
    let temp = tempfile::tempdir().unwrap();
    let code = temp.path().join("code");
    let local = temp.path().join(name);
    std::fs::create_dir_all(&code).unwrap();
    let mut store = PlanningStore::for_code_repository(uuid::Uuid::new_v4(), local, code);
    store.published_spec_pin = Some(PublishedSpecPin {
        repository_id: "primary-code:fixture".into(),
        reference: "refs/heads/main".into(),
        commit_sha: "a".repeat(40),
    });
    (temp, store)
}

fn update(id: &str, content: &str) -> (String, String) {
    (id.to_owned(), content.to_owned())
}

fn persist(store: &PlanningStore, files: &[(String, String)]) {
    let changes = files
        .iter()
        .map(|(path, text)| (path.clone(), text.as_bytes().to_vec()))
        .collect::<Vec<_>>();
    let revision = store.revision().unwrap();
    store.transaction(&changes, Some(&revision)).unwrap();
}

#[test]
fn current_draft_is_durable_and_merges_changes_by_logical_document() {
    let (_temp, store) = store("private");
    assert!(load_current(&store).unwrap().is_none());
    let first = changes(
        &store,
        &[update(
            "product:overview",
            "# Overview\n\nFirst proposal.\n",
        )],
    )
    .unwrap();
    assert_eq!(first.len(), 2);
    assert!(first.iter().any(|(path, _)| path.ends_with("current.json")));
    persist(&store, &first);

    let second = changes(
        &store,
        &[
            update("product:overview", "# Overview\n\nRevised proposal.\n"),
            update(
                "product:architecture-and-constraints",
                "# Architecture and Constraints\n\nSecond document.\n",
            ),
        ],
    )
    .unwrap();
    persist(&store, &second);

    let draft = load_current(&store)
        .unwrap()
        .expect("current proposal reloads");
    assert_eq!(draft.schema_version, 1);
    assert_eq!(draft.base_published_spec_pin, store.published_spec_pin);
    assert_eq!(draft.changes.len(), 2);
    assert_eq!(
        draft.changes[0].document_id,
        "product:architecture-and-constraints"
    );
    assert_eq!(draft.changes[1].document_id, "product:overview");
    assert!(draft.changes[1].content.contains("Revised proposal."));
    assert!(draft.proposal_sha256.len() == 64);
}

#[test]
fn duplicate_updates_are_rejected_without_replacing_the_current_proposal() {
    let (_temp, store) = store("private");
    let duplicate = changes(
        &store,
        &[
            update("product:overview", "# Overview\n\nFirst.\n"),
            update("product:overview", "# Overview\n\nSecond.\n"),
        ],
    );
    assert!(duplicate.is_err());
    assert!(load_current(&store).unwrap().is_none());
}

#[test]
fn malformed_current_projection_is_rejected() {
    let (_temp, store) = store("private");
    store
        .atomic_write("state/spec_drafts/current.json", b"{not-json")
        .unwrap();
    assert!(load_current(&store).is_err());
}

#[test]
fn stale_published_pin_blocks_draft_merge() {
    let (_temp, mut store) = store("private");
    let first = changes(
        &store,
        &[update(
            "product:overview",
            "# Overview\n\nFirst proposal.\n",
        )],
    )
    .unwrap();
    persist(&store, &first);
    store.published_spec_pin.as_mut().unwrap().commit_sha = "b".repeat(40);
    let result = changes(
        &store,
        &[update("product:overview", "# Overview\n\nStale merge.\n")],
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("stale published revision")
    );
}

#[test]
fn snapshot_corruption_and_projection_digest_mismatch_fail_closed() {
    let (_temp, store) = store("private");
    let files = changes(
        &store,
        &[update("product:overview", "# Overview\n\nProposal.\n")],
    )
    .unwrap();
    persist(&store, &files);
    let current_path = store.root.join("state/spec_drafts/current.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&current_path).unwrap()).unwrap();
    value["proposalSha256"] = serde_json::Value::String("f".repeat(64));
    store
        .atomic_write(
            "state/spec_drafts/current.json",
            &serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    assert!(load_current(&store).is_err());
}
