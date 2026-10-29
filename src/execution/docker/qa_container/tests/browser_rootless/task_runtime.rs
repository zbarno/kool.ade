use super::*;

#[path = "task_runtime/polyglot.rs"]
mod polyglot;
#[path = "task_runtime/qa.rs"]
mod qa;
#[path = "task_runtime/runner.rs"]
mod runner;
#[path = "task_runtime/setup.rs"]
mod setup;

/// Boundary: an approved Python task target builds and tests through the
/// app-owned rootless ExecutionSession, records source-bound receipts, and
/// supplies only receipt-verified build files to the isolated browser preview.
/// Unit tests cannot prove the task container, preview relay, browser interaction,
/// and cleanup work together. Dependencies: rootless Docker and the pinned
/// Python/browser QA images. Setup: three short runtime recipes, one build
/// snapshot, and one bounded browser scenario.
#[test]
#[ignore = "requires rootless Docker and the loaded Python/browser QA images"]
fn rootless_approved_task_build_test_start_receipts_feed_browser_qa_artifact() {
    let dependency_request_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let fixture = setup::Fixture::with_required_dependencies(vec![dependency_request_id.clone()]);
    runner::run(fixture, RuntimeProfile::Python, Some(dependency_request_id));
}

/// Production boundary: compile a user-approved Rust project binary in its
/// locked rootless image, run exact-source Build/Test/Start, bind the Start
/// executable to the passed Build artifact, and exercise it with the same
/// browser driver used for the Python fixture. A unit test cannot prove image
/// ABI compatibility, artifact transfer, the task preview, browser interaction,
/// and cleanup together. Dependencies: rootless Docker plus the pinned Rust,
/// Python, and browser QA images. Setup cost: one offline Rust build/test and
/// one bounded browser scenario.
#[test]
#[ignore = "requires rootless Docker and the loaded Rust/Python/browser QA images"]
fn rootless_approved_rust_web_binary_uses_the_same_browser_driver() {
    runner::run(setup::Fixture::rust_web(), RuntimeProfile::Rust, None);
}

/// Production boundary: run an approved TypeScript web target through exact
/// Build/Test/Start receipts, require the selected broker cache in its
/// network-denied Build, and exercise it with the same locked browser driver
/// as Python and Rust. Unit tests cannot prove Node type stripping, cache
/// staging, image compatibility, browser interaction, and cleanup together.
/// Dependencies: rootless Docker plus the pinned Node, Python, and browser QA
/// images. Setup cost: one offline Node build/test and one bounded browser run.
#[test]
#[ignore = "requires rootless Docker and the loaded Node/Python/browser QA images"]
fn rootless_approved_typescript_web_target_uses_the_same_browser_driver() {
    let fixture = setup::Fixture::typescript_web();
    let dependency_request_id = fixture
        .dependency_cache
        .as_ref()
        .expect("TypeScript fixture has a broker-prepared cache")
        .request_id
        .clone();
    runner::run(fixture, RuntimeProfile::Node, Some(dependency_request_id));
}

/// Production boundary: build, test, and start an actual Rust desktop app in
/// the locked Rust executor, prepare its reviewed lifecycle request from the
/// resulting receipts, then run those exact artifact bytes through production
/// desktop dispatch and the locked rootless desktop driver. Unit tests cannot
/// prove executor ABI compatibility, receipt-to-artifact binding, Xvfb window
/// ownership/input, evidence capture, or Docker cleanup together. Dependencies:
/// rootless Docker plus the pinned Rust and desktop QA images. Setup cost: one
/// dependency-free Cargo build/test/start and one bounded desktop scenario.
#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires rootless Docker, the locked Rust image, and the desktop QA candidate"]
fn rootless_approved_rust_desktop_app_uses_locked_desktop_driver() {
    runner::run_rust_desktop(setup::Fixture::rust_desktop());
}

/// Production boundary: authorize the task's exact h1-locked Go module set,
/// stage a known-good GOPATH snapshot through the broker, and build/test/start
/// the API in the network-denied rootless runtime. The Go test runs `go mod
/// verify` offline before the API readiness and service scenarios. Unit tests
/// cannot prove broker-to-cache transfer, Go image compatibility, the internal
/// relay, and cleanup together. Dependencies: rootless Docker and pinned
/// Go/Python/browser QA images. Setup cost: one bounded offline Go build/test
/// and one API scenario.
#[test]
#[ignore = "requires rootless Docker and the loaded Go/Python/browser QA images"]
fn rootless_approved_go_headless_service_uses_headless_driver() {
    let fixture = setup::Fixture::go_headless();
    let dependency_request_id = fixture
        .dependency_cache
        .as_ref()
        .expect("Go fixture has a broker-prepared module cache")
        .request_id
        .clone();
    runner::run(fixture, RuntimeProfile::Go, Some(dependency_request_id));
}

/// Production boundary: run an approved Python HeadlessService through exact
/// Build/Test/Start receipts, the production request and artifact preparation,
/// and the existing rootless API QA driver. It asserts that the driver sees
/// the task-built service and its successful `/save` response. Unit tests
/// cannot prove the Python image, readiness probe, service relay, artifact
/// handoff, and API request together. Dependencies: rootless Docker plus the
/// pinned Python and browser QA images. Setup cost: one offline build/test/start
/// and one bounded API QA scenario.
#[test]
#[ignore = "requires rootless Docker and the loaded Python/browser QA images"]
fn rootless_approved_python_headless_service_uses_headless_driver() {
    runner::run(
        setup::Fixture::python_headless(),
        RuntimeProfile::Python,
        None,
    );
}

/// Production boundary: an approved Python web service passes its `/health`
/// readiness probe, then the rootless browser verifies the user-facing save
/// interaction and must record its failed assertion without granting QA pass.
/// A unit test cannot prove that readiness and browser behavior are checked
/// independently across the real preview relay. Dependencies: rootless Docker
/// plus pinned Python and browser QA images. Setup: one offline build/test,
/// one service launch, one bounded browser scenario, and owned-resource cleanup.
#[test]
#[ignore = "requires rootless Docker and the loaded Python/browser QA images"]
fn rootless_healthy_web_readiness_does_not_mask_failed_browser_assertion() {
    runner::run(
        setup::Fixture::falsely_healthy_python_web(),
        RuntimeProfile::Python,
        None,
    );
}

// The hosted action's isolated socket is below ~/setup-docker-action/run-*/,
// while production environment discovery accepts only native local socket
// locations. Keep this exception in the ignored integration test and connect
// through DockerEndpoint so direct-socket type and owner checks still apply.
fn rootless_action_endpoint() -> crate::execution::docker::engine::DockerEndpoint {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        let host = std::env::var("DOCKER_HOST").expect("rootless Docker action sets DOCKER_HOST");
        let socket = std::path::PathBuf::from(
            host.strip_prefix("unix://")
                .expect("rootless Docker action must expose a Unix socket"),
        );
        assert!(socket.is_absolute());
        let home = std::path::PathBuf::from(
            std::env::var_os("HOME").expect("hosted runner provides HOME before fixture setup"),
        );
        assert_eq!(std::fs::canonicalize(&home).unwrap(), home);
        let run_dir = socket
            .parent()
            .expect("Docker socket has a parent directory");
        let action_dir = home.join("setup-docker-action");
        assert_eq!(run_dir.parent(), Some(action_dir.as_path()));
        assert!(
            run_dir
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.starts_with("run-"))
        );
        let uid = unsafe { libc::geteuid() };
        for directory in [action_dir.as_path(), run_dir] {
            let metadata = std::fs::symlink_metadata(directory)
                .expect("rootless Docker action directory exists");
            assert!(metadata.file_type().is_dir());
            assert_eq!(metadata.uid(), uid);
        }
        assert_eq!(
            socket.file_name(),
            Some(std::ffi::OsStr::new("docker.sock"))
        );
        let metadata = std::fs::symlink_metadata(&socket).expect("rootless Docker socket exists");
        assert!(metadata.file_type().is_socket());
        assert_eq!(metadata.uid(), uid);

        crate::execution::docker::engine::DockerEndpoint::Unix {
            path: socket,
            desktop: false,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        panic!("rootless Docker action endpoint fixture requires Linux")
    }
}

fn allow_rootless_action_socket(
    endpoint: &crate::execution::docker::engine::DockerEndpoint,
) -> crate::test_support::ProcessEnvironmentGuard {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    if let crate::execution::docker::engine::DockerEndpoint::Unix { path, .. } = endpoint {
        environment.set("KOOLADE_TEST_ROOTLESS_ACTION_SOCKET", path.as_os_str());
    }
    environment
}

fn candidate_lock() -> ImageLock {
    let image_reference = std::env::var("KOOLADE_QA_BROWSER_IMAGE")
        .expect("browser QA candidate reference comes from rootless CI");
    let digest = std::env::var("KOOLADE_QA_BROWSER_DIGEST")
        .expect("browser QA candidate digest comes from rootless CI");
    let mut lock = ImageLock::bundled().unwrap();
    let variant = lock
        .profiles
        .get_mut("browser_qa")
        .and_then(|variants| {
            variants
                .iter_mut()
                .find(|variant| variant.platform == "linux/amd64")
        })
        .unwrap();
    variant.reference = image_reference;
    variant.digest = digest;
    variant.runtime_enabled = true;
    lock
}

fn candidate_browser_image(lock: &ImageLock) -> crate::execution::docker::images::SelectedImage {
    lock.select(RuntimeProfile::BrowserQa, "amd64").unwrap()
}
