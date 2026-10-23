use super::*;

impl ExecutionSession {
    pub fn new(
        backend: Arc<dyn ExecutionBackend>,
        workspace: WorkspaceRef,
        target: ExecutionTarget,
        mode: ExecutionModeV1,
        image_digest: String,
        resources: ResourceBudgetV1,
    ) -> anyhow::Result<Self> {
        workspace.validate()?;
        target.validate()?;
        resources.validate()?;
        anyhow::ensure!(
            image_digest.strip_prefix("sha256:").is_some_and(
                |digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
            ),
            "execution session requires an approved image digest"
        );
        Self::new_with_authorization(
            backend,
            workspace,
            target,
            mode,
            image_digest,
            resources,
            None,
        )
    }

    pub fn new_with_authorization(
        backend: Arc<dyn ExecutionBackend>,
        workspace: WorkspaceRef,
        target: ExecutionTarget,
        mode: ExecutionModeV1,
        image_digest: String,
        resources: ResourceBudgetV1,
        authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
    ) -> anyhow::Result<Self> {
        Self::new_with_destination_authorization(
            backend,
            workspace,
            target,
            mode,
            image_digest,
            resources,
            None,
            authorization_verifier,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_destination_authorization(
        backend: Arc<dyn ExecutionBackend>,
        workspace: WorkspaceRef,
        target: ExecutionTarget,
        mode: ExecutionModeV1,
        image_digest: String,
        resources: ResourceBudgetV1,
        destination_sha: Option<String>,
        authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
    ) -> anyhow::Result<Self> {
        workspace.validate()?;
        anyhow::ensure!(
            destination_sha
                .as_deref()
                .is_none_or(crate::execution::contract::is_valid_sha),
            "execution session destination SHA is invalid"
        );
        target.validate()?;
        resources.validate()?;
        anyhow::ensure!(
            image_digest.strip_prefix("sha256:").is_some_and(
                |digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
            ),
            "execution session requires an approved image digest"
        );
        let authorization_generation = authorization_verifier
            .as_ref()
            .map(|verifier| verifier.generation())
            .unwrap_or_else(|| uuid::Uuid::new_v4().hyphenated().to_string());
        crate::execution::contract::validate_identifier(&authorization_generation)?;
        let runtime = make_runtime()?;
        Ok(Self {
            backend,
            workspace,
            destination_sha,
            target,
            mode,
            image_digest: std::sync::RwLock::new(image_digest),
            derived_image: std::sync::Mutex::new(None),
            authorization_generation,
            authorization_verifier,
            remote_retry_factory: Mutex::new(None),
            retry_destination: Mutex::new(None),
            resources,
            dependency_cache_refs: Mutex::new(Vec::new()),
            prepared_dependency_ids: Mutex::new(BTreeSet::new()),
            remote_dependency_requests: Mutex::new(std::collections::BTreeMap::new()),
            remote_telemetry_attribution: Mutex::new(None),
            sequence: AtomicU64::new(1),
            cancellation_generation: AtomicU64::new(1),
            busy: AtomicBool::new(false),
            cancel_requested: AtomicBool::new(false),
            active: Mutex::new(None),
            remote_capacity_lease: Mutex::new(None),
            runtime_processes: Mutex::new(Vec::new()),
            runtime,
        })
    }
}
