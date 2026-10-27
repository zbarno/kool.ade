use std::{collections::BTreeSet, fs};

use crate::{
    execution::docker::images::RuntimeProfile,
    persistence::setup_tasks::{SetupTaskStoreV1, SetupTaskTransitionV1},
};

#[test]
fn selected_profiles_are_persisted_as_a_bounded_set() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("machine-setup").join("tasks.json");
    for profile in [
        RuntimeProfile::General,
        RuntimeProfile::Rust,
        RuntimeProfile::Node,
        RuntimeProfile::Python,
        RuntimeProfile::Go,
    ] {
        super::super::transaction::apply_transition_at(
            &path,
            SetupTaskTransitionV1::RequireImageProfile { profile },
        )
        .unwrap();
    }
    let (store, _) = super::super::transaction::load_from(&path).unwrap();
    assert_eq!(
        store.required_image_profiles,
        BTreeSet::from([
            RuntimeProfile::General,
            RuntimeProfile::Rust,
            RuntimeProfile::Node,
            RuntimeProfile::Python,
            RuntimeProfile::Go,
        ])
    );
}

#[test]
fn qa_profiles_cannot_be_recorded_as_bootstrap_requirements() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("machine-setup").join("tasks.json");
    assert!(
        super::super::transaction::apply_transition_at(
            &path,
            SetupTaskTransitionV1::RequireImageProfile {
                profile: RuntimeProfile::BrowserQa,
            },
        )
        .is_err()
    );

    let malformed = SetupTaskStoreV1 {
        required_image_profiles: BTreeSet::from([RuntimeProfile::DesktopQa]),
        ..SetupTaskStoreV1::default()
    };
    assert!(malformed.validate().is_err());
}

#[test]
fn old_task_store_without_profile_requirements_loads_empty() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tasks.json");
    fs::write(
        &path,
        br#"{"schemaVersion":2,"latestScanGeneration":0,"tasks":{}}"#,
    )
    .unwrap();

    let (store, migrated) = super::super::transaction::load_from(&path).unwrap();
    assert!(!migrated);
    assert!(store.required_image_profiles.is_empty());
}

#[test]
fn store_validation_rejects_more_than_the_supported_profiles() {
    let store = SetupTaskStoreV1 {
        required_image_profiles: BTreeSet::from([
            RuntimeProfile::General,
            RuntimeProfile::Rust,
            RuntimeProfile::Node,
            RuntimeProfile::Python,
            RuntimeProfile::Go,
            RuntimeProfile::BrowserQa,
        ]),
        ..SetupTaskStoreV1::default()
    };
    assert!(store.validate().is_err());
}
