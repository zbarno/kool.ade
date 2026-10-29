use std::{collections::BTreeSet, path::Path};

use crate::{
    execution::{
        contract::ResourceBudgetV1,
        docker::images::{ImageLock, SelectedImage},
    },
    runtime_qa::QaDriverActionV1,
};

// Production boundary: the app-owned rootless desktop driver must accept a
// PACKAGED EXECUTABLE COMPILED FROM A NON-RUST TOOLCHAIN through the same
// approved-artifact flow (hash receipt, tmpfs staging, descendant-ownership
// filtering) as the Rust reference fixture, and its diagnostics must keep the
// target-capability vocabulary when the artifact cannot start (missing loader
// dependency) or crash mid-scenario. The happy path also sends double-click,
// drag, key chord, and scroll input through the actual rootless Xvfb boundary;
// each target state change is observed before the next action.
// Unit tests cannot prove Docker upload/staging, Xvfb pointer/keyboard
// delivery for double-click, drag, key-chord, and scroll, crash observation,
// active driver cancellation, and cleanup work together for a foreign ABI
// artifact; the focus-before-click proof rides on the fixture dropping keys
// unless the driver focused its window first (no window manager in the image).
// External dependency: rootless Docker with the just-loaded desktop candidate
// and the separately compiled C/Xlib fixtures. Setup cost: one `cc` build of
// the two fixtures (headers: libx11-dev) plus four short Xvfb container runs.
pub(super) async fn exercise(
    docker: &bollard::Docker,
    image: &SelectedImage,
    lock: &ImageLock,
    resources: &ResourceBudgetV1,
) {
    let fixture_path = match std::env::var_os("KOOLADE_QA_NONRUST_FIXTURE_EXECUTABLE") {
        Some(path) => path,
        None if std::env::var_os("KOOLADE_QA_REQUIRE_NON_RUST_ARTIFACT").is_some() => {
            panic!(
                "the rootless candidate job requires its separately built non-Rust desktop fixture"
            )
        }
        None => return,
    };
    let fixture_path = Path::new(&fixture_path);
    scenarios::require_approved_executable(fixture_path);
    let missing_raw = match std::env::var_os("KOOLADE_QA_NONRUST_MISSING_DEP_EXECUTABLE") {
        Some(raw) => Some(raw),
        None if std::env::var_os("KOOLADE_QA_REQUIRE_NON_RUST_ARTIFACT").is_some() => {
            panic!(
                "the rootless candidate job requires its separately built missing-dependency fixture"
            )
        }
        None => None,
    };

    let response = scenarios::run_scenario(
        docker,
        image,
        lock,
        resources,
        fixture_path,
        scenarios::happy_steps(),
        "happy",
    )
    .await;
    assert_eq!(response["completed"], true, "{response}");
    assert_eq!(response["failure"], serde_json::Value::Null, "{response}");
    assert_eq!(
        response["actions"].as_array().unwrap().len(),
        21,
        "{response}"
    );
    assert_eq!(
        response["screenshots"].as_array().unwrap().len(),
        2,
        "{response}"
    );
    let assertions = response["assertions"].as_array().unwrap();
    assert_eq!(assertions.len(), 20, "{response}");
    assert!(
        assertions
            .iter()
            .all(|assertion| assertion["status"] == "passed"),
        "{response}"
    );
    let commands = response["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|action| action["command"].as_str())
        .collect::<BTreeSet<_>>();
    for command in [
        "double_click",
        "drag",
        "focus_window",
        "key_chord",
        "scroll",
    ] {
        assert!(
            commands.contains(command),
            "missing {command} evidence: {response}"
        );
    }

    let response = scenarios::run_scenario(
        docker,
        image,
        lock,
        resources,
        fixture_path,
        scenarios::crash_steps(),
        "crash",
    )
    .await;
    assert_eq!(response["completed"], false, "{response}");
    let failure = response["failure"].as_str().unwrap_or_default();
    assert!(
        failure.contains("approved application exited before the scenario completed"),
        "{response}"
    );
    assert_eq!(
        response["actions"].as_array().unwrap().len(),
        3,
        "{response}"
    );
    assert_eq!(
        response["screenshots"].as_array().unwrap().len(),
        2,
        "{response}"
    );
    assert!(
        response["crashes"]
            .as_array()
            .is_some_and(|crashes| !crashes.is_empty()),
        "{response}"
    );

    if let Some(missing_raw) = missing_raw {
        let missing_path = Path::new(missing_raw.as_os_str());
        scenarios::require_approved_executable(missing_path);
        let response = scenarios::run_scenario(
            docker,
            image,
            lock,
            resources,
            missing_path,
            vec![(QaDriverActionV1::LaunchValidatedTarget, None, false)],
            "missing-dep",
        )
        .await;
        assert_eq!(response["completed"], false, "{response}");
        let failure = response["failure"].as_str().unwrap_or_default();
        // Structured diagnostic: an unresolvable NEEDED entry must be reported
        // as a missing dependency, never swallowed into a passing launch.
        assert!(failure.contains("missing_dependencies"), "{response}");
        assert_eq!(
            response["actions"].as_array().unwrap().len(),
            1,
            "{response}"
        );
        assert!(
            response["screenshots"]
                .as_array()
                .is_none_or(|shots| shots.is_empty()),
            "{response}"
        );
        assert!(
            response["assertions"]
                .as_array()
                .is_none_or(|items| items.is_empty()),
            "{response}"
        );
    }

    scenarios::exercise_cancellation(docker, image, lock, resources, fixture_path).await;
}

#[path = "non_rust_artifact/scenarios.rs"]
mod scenarios;
