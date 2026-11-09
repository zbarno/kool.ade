//! Project-aware local session construction.
//!
//! The implementation path prefers the operator-approved project reality
//! (capability manifest recipes) over filename heuristics: the session
//! connects to the first locked image providing every required logical
//! tool, or to a verified on-demand composite when no single locked image
//! covers the approved polyglot set.

use std::{path::Path, sync::Arc};

use crate::execution::{
    contract::{ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
    docker::images::{RuntimePlanBinding, RuntimeProfile},
    errors::ExecutionError,
};

use super::local::connect_local_with_image_selection;
use super::{AuthorizationGenerationVerifier, ExecutionSession};

impl ExecutionSession {
    /// Creates a local task session from approved project recipe
    /// requirements, binding the runtime plan to the project facts that
    /// authorized it (`plan_binding`) for audit provenance.
    #[allow(clippy::too_many_arguments)]
    pub fn connect_local_with_destination_tools_authorization(
        workspace: WorkspaceRef,
        task_clone: &Path,
        mode: ExecutionModeV1,
        required_tools: &[String],
        resources: ResourceBudgetV1,
        destination_sha: Option<String>,
        authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
        plan_binding: Option<RuntimePlanBinding>,
    ) -> Result<Self, ExecutionError> {
        const PROFILES: &[RuntimeProfile] = &[
            RuntimeProfile::General,
            RuntimeProfile::Rust,
            RuntimeProfile::Node,
            RuntimeProfile::Python,
            RuntimeProfile::Go,
        ];
        connect_local_with_image_selection(
            workspace,
            task_clone,
            mode,
            PROFILES,
            required_tools,
            resources,
            destination_sha,
            authorization_verifier,
            plan_binding.as_ref(),
        )
    }
}
