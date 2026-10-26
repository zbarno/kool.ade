use crate::app::{
    bootstrap_remediation::claude_code_install::ClaudeInstallProgress,
    bootstrap_remediation::{
        plan::{PrivilegeLevel, RemediationPlan},
        supervisor::{OperationRegistry, TrustedInvocation},
    },
    bootstrap_repair_policy::Operation,
};
use std::sync::mpsc::Sender;

pub(crate) struct ClaudeCodeOperationRegistry {
    progress: Sender<ClaudeInstallProgress>,
}

impl ClaudeCodeOperationRegistry {
    pub(crate) fn new(progress: Sender<ClaudeInstallProgress>) -> Self {
        Self { progress }
    }
}

impl OperationRegistry for ClaudeCodeOperationRegistry {
    fn compile(&self, plan: &RemediationPlan) -> anyhow::Result<TrustedInvocation> {
        plan.validate()?;
        plan.validate_current_target()?;
        anyhow::ensure!(
            plan.operation == Operation::InstallClaudeCode
                && plan.privilege == PrivilegeLevel::CurrentUser,
            "Claude registry only compiles the reviewed current-user install"
        );
        let target = plan
            .install_target
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Claude install target is missing"))?;
        TrustedInvocation::claude_install(std::path::PathBuf::from(target), self.progress.clone())
    }
}
