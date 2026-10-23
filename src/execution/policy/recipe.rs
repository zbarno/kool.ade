use super::{
    ApprovedRecipeCommandV1, ExecutionModeV1, ExecutionPolicyV1, FilesystemGrantV1,
    ResourceBudgetV1, validation::validate_environment_allowlist,
};
use sha2::Digest;
use std::collections::BTreeSet;

impl ExecutionPolicyV1 {
    pub(crate) fn for_scoped_runtime_launcher(
        resource_limits: ResourceBudgetV1,
        root_relative: String,
        authorization_ref: String,
        script: &str,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            valid_recipe_authorization_ref(&authorization_ref),
            "runtime launcher requires verified recipe authorization"
        );
        let mut policy = Self::for_scoped_workspace_write(
            ExecutionModeV1::Verification,
            resource_limits,
            root_relative,
        )?;
        policy.environment_names.clear();
        policy.command_capabilities.insert("sh".into());
        policy.approved_recipe_command = Some(ApprovedRecipeCommandV1 {
            approval_ref: authorization_ref,
            executable_id: "sh".into(),
            argv: vec!["-s".into()],
            working_directory_relative: ".".into(),
            allowed_environment_names: BTreeSet::new(),
            stdin_script_sha256: Some(format!("{:x}", sha2::Sha256::digest(script.as_bytes()))),
        });
        policy.validate()?;
        Ok(policy)
    }

    pub(crate) fn without_scoped_workspace_write(mut self) -> Self {
        self.filesystem
            .remove(&FilesystemGrantV1::WorkspaceScopedWrite);
        self
    }

    pub fn for_approved_recipe(
        resource_limits: ResourceBudgetV1,
        root_relative: String,
        approval_ref: String,
        executable_id: String,
        argv: Vec<String>,
        working_directory_relative: String,
        allowed_environment_names: BTreeSet<String>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            approval_ref.starts_with("user-approval:") && approval_ref.len() <= 2048,
            "recipe command requires an explicit user approval receipt"
        );
        Self::for_authorized_recipe_command(
            resource_limits,
            root_relative,
            approval_ref,
            executable_id,
            argv,
            working_directory_relative,
            allowed_environment_names,
        )
    }

    pub(crate) fn for_trusted_adapter_recipe(
        resource_limits: ResourceBudgetV1,
        root_relative: String,
        provenance_ref: String,
        executable_id: String,
        argv: Vec<String>,
        working_directory_relative: String,
        allowed_environment_names: BTreeSet<String>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            provenance_ref.starts_with("trusted-adapter-provenance:")
                && valid_recipe_authorization_ref(&provenance_ref),
            "trusted adapter recipe requires verified local provenance"
        );
        Self::for_authorized_recipe_command(
            resource_limits,
            root_relative,
            provenance_ref,
            executable_id,
            argv,
            working_directory_relative,
            allowed_environment_names,
        )
    }

    fn for_authorized_recipe_command(
        resource_limits: ResourceBudgetV1,
        root_relative: String,
        authorization_ref: String,
        executable_id: String,
        argv: Vec<String>,
        working_directory_relative: String,
        allowed_environment_names: BTreeSet<String>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !matches!(
                executable_id.as_str(),
                "sh" | "bash" | "pwsh" | "powershell"
            ),
            "approved recipes cannot select a command interpreter"
        );
        anyhow::ensure!(
            !executable_id.is_empty()
                && executable_id.len() <= 128
                && executable_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')),
            "approved recipe executable is invalid"
        );
        anyhow::ensure!(
            !argv.is_empty()
                && argv.len() <= 128
                && argv
                    .iter()
                    .all(|arg| arg.len() <= 4096 && !arg.contains('\0')),
            "approved recipe arguments are invalid"
        );
        let mut policy = Self::for_scoped_workspace_write(
            ExecutionModeV1::Verification,
            resource_limits,
            root_relative,
        )?;
        policy.command_capabilities.insert(executable_id.clone());
        policy.environment_names = allowed_environment_names.clone();
        validate_environment_allowlist(&policy.environment_names)?;
        policy.approved_recipe_command = Some(ApprovedRecipeCommandV1 {
            approval_ref: authorization_ref,
            executable_id,
            argv,
            working_directory_relative,
            allowed_environment_names,
            stdin_script_sha256: None,
        });
        Ok(policy)
    }
}

pub(super) fn valid_recipe_authorization_ref(reference: &str) -> bool {
    if let Some(receipt) = reference.strip_prefix("user-approval:") {
        return !receipt.trim().is_empty() && reference.len() <= 2048;
    }
    reference
        .strip_prefix("trusted-adapter-provenance:")
        .is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}
