use std::path::PathBuf;

use crate::{
    app::session::Project, artifacts::planning_store::PlanningStore, core::lifecycle::TaskRecord,
    execution::contract::WorkspaceRef, project_runtime::AppSurface,
};

#[path = "setup/desktop_lifecycle.rs"]
pub(super) mod desktop_lifecycle;
#[path = "setup/falsely_healthy_web.rs"]
mod falsely_healthy_web;
#[path = "setup/fixture.rs"]
mod fixture;
#[path = "setup/git.rs"]
mod git;
#[path = "setup/go_headless.rs"]
mod go_headless;
#[path = "setup/npm_fixture.rs"]
pub(super) mod npm_fixture;
#[path = "setup/prepared_cache.rs"]
pub(super) mod prepared_cache;
#[path = "setup/prepared_go_cache.rs"]
mod prepared_go_cache;
#[path = "setup/python_headless.rs"]
mod python_headless;
#[path = "setup/recipes.rs"]
mod recipes;
#[path = "setup/rust_desktop.rs"]
mod rust_desktop;
#[path = "setup/rust_web.rs"]
mod rust_web;
#[path = "setup/typescript_web.rs"]
mod typescript_web;
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FixtureKind {
    PythonWeb,
    FalselyHealthyPythonWeb,
    RustWeb,
    TypeScriptWeb,
    RustDesktop,
    PythonHeadless,
    GoHeadless,
}

impl FixtureKind {
    fn language(self) -> &'static str {
        match self {
            Self::PythonWeb => "python",
            Self::FalselyHealthyPythonWeb => "python",
            Self::RustWeb => "rust",
            Self::TypeScriptWeb => "typescript",
            Self::RustDesktop => "rust",
            Self::PythonHeadless => "python",
            Self::GoHeadless => "go",
        }
    }

    fn fixture_ref(self) -> &'static str {
        match self {
            Self::PythonWeb => "python-task-web-v1",
            Self::FalselyHealthyPythonWeb => "python-falsely-healthy-web-v1",
            Self::RustWeb => "rust-binary-task-web-v1",
            Self::TypeScriptWeb => "typescript-task-web-v1",
            Self::RustDesktop => "rust-native-desktop-task-v1",
            Self::PythonHeadless => "python-headless-api-v1",
            Self::GoHeadless => "go-headless-api-v1",
        }
    }

    fn artifacts(self) -> Vec<String> {
        match self {
            Self::PythonWeb => vec!["dist/app.py".into(), "dist/index.html".into()],
            Self::FalselyHealthyPythonWeb => {
                vec!["dist/app.py".into(), "dist/index.html".into()]
            }
            Self::RustWeb => vec!["target/release/runtime-qa-rust-web".into()],
            Self::TypeScriptWeb => vec!["dist/server.ts".into(), "dist/index.html".into()],
            Self::RustDesktop => vec!["target/release/runtime-qa-rust-desktop".into()],
            Self::PythonHeadless => vec!["dist/app.py".into()],
            Self::GoHeadless => vec!["dist/runtime-qa-go-api".into()],
        }
    }

    fn app_surface(self) -> AppSurface {
        match self {
            Self::PythonWeb
            | Self::FalselyHealthyPythonWeb
            | Self::RustWeb
            | Self::TypeScriptWeb => AppSurface::Web,
            Self::RustDesktop => AppSurface::LinuxDesktop,
            Self::PythonHeadless => AppSurface::HeadlessService,
            Self::GoHeadless => AppSurface::HeadlessService,
        }
    }
}

pub(super) struct Fixture {
    pub projects_root: PathBuf,
    pub task_clone: PathBuf,
    pub planning: PlanningStore,
    pub project: Option<Project>,
    pub lifecycle_task: Option<TaskRecord>,
    pub workspace: WorkspaceRef,
    pub target_id: String,
    pub build_recipe_id: String,
    pub test_recipe_id: String,
    pub start_recipe_id: String,
    pub dependency_cache: Option<prepared_cache::SeededCache>,
    pub readiness_path: String,
    pub expect_browser_failure: bool,
    _approval_cleanup: crate::persistence::runtime_target_approvals::WorkspaceCleanup,
    _home: crate::test_support::KooladeHomeGuard,
    pub root: tempfile::TempDir,
}

impl Fixture {
    pub fn with_required_dependencies(required_dependencies: Vec<String>) -> Self {
        Self::with_kind(required_dependencies, FixtureKind::PythonWeb)
    }

    pub fn rust_web() -> Self {
        Self::with_kind(Vec::new(), FixtureKind::RustWeb)
    }

    pub fn typescript_web() -> Self {
        Self::with_kind(Vec::new(), FixtureKind::TypeScriptWeb)
    }

    pub fn rust_desktop() -> Self {
        Self::with_kind(Vec::new(), FixtureKind::RustDesktop)
    }

    pub fn go_headless() -> Self {
        Self::with_kind(Vec::new(), FixtureKind::GoHeadless)
    }

    pub fn python_headless() -> Self {
        Self::with_kind(Vec::new(), FixtureKind::PythonHeadless)
    }

    pub fn falsely_healthy_python_web() -> Self {
        Self::with_kind(Vec::new(), FixtureKind::FalselyHealthyPythonWeb)
    }

    fn with_kind(required_dependencies: Vec<String>, kind: FixtureKind) -> Self {
        fixture::create_fixture(required_dependencies, kind)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
