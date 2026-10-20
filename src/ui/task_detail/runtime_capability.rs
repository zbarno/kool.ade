#[derive(Clone)]
pub struct RuntimeCapabilityView {
    pub source_sha: Option<String>,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub manifest_revision: Option<u64>,
    pub selected_target_id: Option<String>,
    pub targets: Vec<RuntimeTargetChoice>,
    pub prepared_dependencies: Vec<PreparedDependencyChoice>,
    pub message: Option<String>,
}

#[derive(Clone)]
pub struct PreparedDependencyChoice {
    pub request_id: String,
    pub label: String,
}

#[derive(Clone)]
pub struct RuntimeTargetChoice {
    pub target_id: String,
    pub label: String,
    pub detected_language: Option<String>,
    pub app_surface: crate::project_runtime::AppSurface,
    pub selected: bool,
    pub state: String,
    pub recipes: String,
    pub required_dependencies: Vec<String>,
    pub qa_applicability: crate::project_runtime_capability::QaApplicability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeRecipeOperation {
    Build,
    Test,
    Start,
}

#[derive(Clone, Default)]
pub struct RuntimeRecipeInput {
    pub executable_id: String,
    pub argv_lines: String,
}
