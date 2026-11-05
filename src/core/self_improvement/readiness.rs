use super::SelfImprovementRunV1;

impl SelfImprovementRunV1 {
    /// Produce lookup readiness only by running the registered probe against
    /// this exact installed build. Manager output cannot author this evidence.
    pub fn capability_readiness_evidence(
        &self,
    ) -> Result<crate::core::product_capabilities::ReadinessEvidence, String> {
        let (version, build_commit) = crate::build_identity::current()
            .ok_or_else(|| "active application build identity is unavailable".to_owned())?;
        if self.installed_version.as_deref() != Some(version)
            || self.installed_build_commit_sha.as_deref() != Some(build_commit)
        {
            return Err("active application build differs from the recorded installation".into());
        }
        let proof = super::capability_probe::probe_compiled_capability(self)?;
        crate::core::product_capabilities::ReadinessEvidence::from_registered_probe(
            proof.capability_id,
            proof.readiness_probe_id,
            build_commit.to_owned(),
            proof.manifest_sha256,
        )
    }
}
