use std::{collections::BTreeMap, fs};

use super::super::{SetupTaskStateV1, transaction};

#[test]
fn schema_one_migration_keeps_recent_history_and_rechecks_unverifiable_legacy_success() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tasks.json");
    let events = (0..30)
        .map(|at| format!(r#"{{"observedAtUnixMs":{at},"state":"needs_attention"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let legacy = format!(
        r#"{{
          "schemaVersion":1,
          "tasks":{{
            "setup:docker":{{
              "id":"setup:docker","title":"Docker setup","explanation":"Legacy notice",
              "nextStep":"Check Docker","prerequisites":[],"state":"needs_attention",
              "observedAtUnixMs":30,"verifiedAtUnixMs":null,"events":[{events}]
            }},
            "setup:provider":{{
              "id":"setup:provider","title":"Provider setup","explanation":"Legacy notice",
              "nextStep":"Check provider","prerequisites":[],"state":"verified",
              "observedAtUnixMs":40,"verifiedAtUnixMs":40,
              "events":[{{"observedAtUnixMs":40,"state":"verified"}}]
            }}
          }}
        }}"#
    );
    fs::write(&path, legacy).unwrap();

    let (store, migrated) = transaction::load_from(&path).unwrap();
    assert!(migrated);
    assert_eq!(
        store.tasks["setup:legacy:docker"]
            .legacy_source_id
            .as_deref(),
        Some("setup:docker")
    );
    assert_eq!(
        store.tasks["setup:legacy:docker"].state,
        SetupTaskStateV1::NeedsVerification
    );
    assert_eq!(
        store.tasks["setup:legacy:provider"].state,
        SetupTaskStateV1::NeedsVerification
    );
    assert_eq!(store.tasks["setup:legacy:docker"].events.len(), 24);
    assert_eq!(
        store.tasks["setup:legacy:docker"].events[0].observed_at_unix_ms,
        6
    );
    assert!(
        store.tasks["setup:legacy:provider"]
            .verified_result
            .is_none()
    );

    transaction::update_at(&path, |store| Ok((store.clone(), false))).unwrap();
    let encoded = fs::read_to_string(&path).unwrap();
    assert!(encoded.contains("\"schemaVersion\": 2"));
    assert!(encoded.contains("setup:legacy:docker"));
}

#[test]
fn migration_collision_is_rejected_without_overwriting_the_source_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tasks.json");
    let mut tasks = BTreeMap::new();
    for id in ["setup:docker", "setup:legacy:docker"] {
        tasks.insert(
            id,
            serde_json::json!({
                "id": id,
                "title": "Synthetic legacy task",
                "explanation": "Synthetic legacy data",
                "nextStep": "Recheck",
                "prerequisites": [],
                "state": "needs_attention",
                "observedAtUnixMs": 1,
                "verifiedAtUnixMs": null,
                "events": []
            }),
        );
    }
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 1,
        "tasks": tasks
    }))
    .unwrap();
    fs::write(&path, &bytes).unwrap();

    assert!(transaction::load_from(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
