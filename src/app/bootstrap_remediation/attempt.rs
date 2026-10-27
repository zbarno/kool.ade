use crate::{
    app::{
        bootstrap_remediation::identity::HostIdentity, bootstrap_remediation::plan::RemediationPlan,
    },
    persistence::setup_tasks::{self, SetupTaskStoreV1, SetupTaskTransitionV1},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttemptContext {
    pub(super) task_id: String,
    pub(super) operation_id: String,
    pub(super) owner_instance_id: String,
    pub(super) requested_scopes: Vec<String>,
    pub(super) scope_digest: String,
}

impl AttemptContext {
    pub(crate) fn begin(plan: &RemediationPlan) -> anyhow::Result<Self> {
        let identity = HostIdentity::current()?;
        let now = super::approval::now_unix_ms()?;
        plan.validate()?;
        identity.validate()?;
        anyhow::ensure!(
            plan.host == identity.host() && plan.os_version == identity.os_version(),
            "remediation plan targets another host OS"
        );
        anyhow::ensure!(
            identity.matches_current()?,
            "host identity changed before remediation attempt"
        );
        let requested_scopes = bound_scopes(plan, &identity)?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let scope_digest = setup_tasks::authorization_scope_digest(
            &plan.prerequisite_id,
            &operation_id,
            plan.operation_code_version,
            &requested_scopes,
        );
        let store = setup_tasks::apply_transition(SetupTaskTransitionV1::RequestAttempt {
            task_id: plan.prerequisite_id.clone(),
            operation_id: operation_id.clone(),
            version: plan.operation_code_version,
            requested_scopes: requested_scopes.clone(),
            at_unix_ms: now,
        })?;
        let owner_instance_id = store
            .tasks
            .get(&plan.prerequisite_id)
            .and_then(|task| {
                task.attempts
                    .iter()
                    .find(|attempt| attempt.operation_id == operation_id)
            })
            .and_then(|attempt| attempt.owner_instance_id.clone())
            .ok_or_else(|| anyhow::anyhow!("durable remediation attempt has no owner"))?;
        Ok(Self {
            task_id: plan.prerequisite_id.clone(),
            operation_id,
            owner_instance_id,
            scope_digest,
            requested_scopes,
        })
    }

    #[cfg(test)]
    pub(crate) fn begin_for_test(
        store: &mut SetupTaskStoreV1,
        plan: &RemediationPlan,
        identity: &HostIdentity,
        owner_instance_id: &str,
        at_unix_ms: u64,
    ) -> anyhow::Result<Self> {
        plan.validate()?;
        identity.validate()?;
        anyhow::ensure!(
            plan.host == identity.host() && plan.os_version == identity.os_version(),
            "remediation plan targets another host"
        );
        anyhow::ensure!(
            plan.prerequisite_id.starts_with("setup:"),
            "remediation plan lacks a setup task identity"
        );
        let task_id = plan.prerequisite_id.clone();
        let operation_id = uuid::Uuid::new_v4().to_string();
        let requested_scopes = bound_scopes(plan, identity)?;
        let scope_digest = setup_tasks::authorization_scope_digest(
            &task_id,
            &operation_id,
            plan.operation_code_version,
            &requested_scopes,
        );
        store.request_attempt(
            &task_id,
            operation_id.clone(),
            plan.operation_code_version,
            requested_scopes.clone(),
            owner_instance_id,
            at_unix_ms,
        )?;
        Ok(Self {
            task_id,
            operation_id,
            owner_instance_id: owner_instance_id.to_owned(),
            requested_scopes,
            scope_digest,
        })
    }

    pub(crate) fn persist_authorization(
        &self,
        binding: crate::persistence::setup_tasks::AuthorizationBindingV1,
    ) -> anyhow::Result<SetupTaskStoreV1> {
        anyhow::ensure!(
            binding.scope_digest == self.scope_digest,
            "approval receipt has the wrong remediation scope"
        );
        setup_tasks::apply_transition(SetupTaskTransitionV1::RecordAuthorization {
            task_id: self.task_id.clone(),
            operation_id: self.operation_id.clone(),
            binding,
            at_unix_ms: super::approval::now_unix_ms()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn persist_authorization_for_test(
        &self,
        store: &mut SetupTaskStoreV1,
        binding: crate::persistence::setup_tasks::AuthorizationBindingV1,
        at_unix_ms: u64,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            binding.scope_digest == self.scope_digest,
            "approval receipt has the wrong remediation scope"
        );
        store.authorize_attempt(
            &self.task_id,
            &self.operation_id,
            binding,
            &self.owner_instance_id,
            at_unix_ms,
        )
    }

    pub(crate) fn task_id(&self) -> &str {
        &self.task_id
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) fn owner_instance_id(&self) -> &str {
        &self.owner_instance_id
    }

    pub(crate) fn scope_digest(&self) -> &str {
        &self.scope_digest
    }

    pub(super) fn requested_scopes(&self) -> &[String] {
        &self.requested_scopes
    }
}

pub(super) fn bound_scopes(
    plan: &RemediationPlan,
    identity: &HostIdentity,
) -> anyhow::Result<Vec<String>> {
    let plan_digest = plan.digest()?;
    let host_digest = identity.digest()?;
    let mut scopes = vec![
        plan.prerequisite_id.clone(),
        format!(
            "setup:remediation:operation:{}",
            operation_code(plan.operation)
        ),
        format!("setup:remediation:plan:{plan_digest}"),
        format!("setup:remediation:host:{host_digest}"),
    ];
    for prerequisite in &plan.verification_probe_ids {
        let scope = if prerequisite.starts_with("setup:") {
            prerequisite.clone()
        } else {
            format!("setup:probe:{prerequisite}")
        };
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    anyhow::ensure!(
        scopes.len() <= 24,
        "remediation plan has too many authorization scopes"
    );
    Ok(scopes)
}

fn operation_code(operation: crate::app::bootstrap_repair_policy::Operation) -> &'static str {
    use crate::app::bootstrap_repair_policy::Operation;
    match operation {
        Operation::RecheckPrerequisites => "recheck_prerequisites",
        Operation::PrepareLockedImage => "prepare_locked_image",
        Operation::OpenCodingTools => "open_coding_tools",
        Operation::InstallLinuxRootlessPackages => "install_linux_rootless_packages",
        Operation::ConfigureLinuxSubordinateIds => "configure_linux_subordinate_ids",
        Operation::InstallLinuxRootlessService => "install_linux_rootless_service",
    }
}
