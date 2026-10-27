use std::path::PathBuf;

pub(crate) const DNF_MAX_STAGED_RPMS: usize = 192;

/// Random binding for one root-owned Fedora package staging run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DnfStageId(String);

impl DnfStageId {
    pub(crate) fn new() -> Self {
        Self(uuid::Uuid::new_v4().simple().to_string())
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.0.len() == 32
                && self
                    .0
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "DNF stage identifier is malformed"
        );
        Ok(())
    }

    pub(crate) fn package(&self, basename: &str) -> anyhow::Result<StagedRpm> {
        self.validate()?;
        anyhow::ensure!(
            valid_rpm_basename(basename),
            "staged RPM basename is malformed"
        );
        Ok(StagedRpm {
            stage: self.clone(),
            basename: basename.to_owned(),
        })
    }

    pub(super) fn root_path(&self) -> PathBuf {
        PathBuf::from(format!("/run/koolade-rootless-dnf-{}", self.0))
    }

    pub(super) fn packages_path(&self) -> PathBuf {
        self.root_path().join("packages")
    }

    pub(super) fn rpmdb_path(&self) -> PathBuf {
        self.root_path().join("rpmdb")
    }

    pub(super) fn cache_path(&self) -> PathBuf {
        self.root_path().join("cache")
    }

    pub(super) fn binding_id(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StagedRpm {
    stage: DnfStageId,
    basename: String,
}

impl StagedRpm {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        self.stage.validate()?;
        anyhow::ensure!(
            valid_rpm_basename(&self.basename),
            "staged RPM basename is malformed"
        );
        Ok(())
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.stage.packages_path().join(&self.basename)
    }

    pub(crate) fn basename(&self) -> &str {
        &self.basename
    }

    pub(crate) fn stage(&self) -> &DnfStageId {
        &self.stage
    }

    pub(crate) fn binding_value(&self) -> String {
        format!("{}:{}", self.stage.binding_id(), self.basename)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HashBoundDnfRpm {
    package: StagedRpm,
    sha256: String,
}

impl HashBoundDnfRpm {
    pub(crate) fn new(package: StagedRpm, sha256: String) -> anyhow::Result<Self> {
        package.validate()?;
        anyhow::ensure!(
            sha256.len() == 64
                && sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "staged RPM SHA-256 digest is malformed"
        );
        Ok(Self { package, sha256 })
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        Self::new(self.package.clone(), self.sha256.clone()).map(|_| ())
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.package.path()
    }

    pub(crate) fn package(&self) -> &StagedRpm {
        &self.package
    }

    pub(crate) fn stage(&self) -> &DnfStageId {
        self.package.stage()
    }

    pub(in crate::app::bootstrap_remediation::supervisor::process) fn binding_value(
        &self,
    ) -> String {
        format!("{}:{}", self.package.binding_value(), self.sha256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedDnfRpm(HashBoundDnfRpm);

impl VerifiedDnfRpm {
    pub(crate) fn after_rpmkeys_success(package: HashBoundDnfRpm) -> Self {
        Self(package)
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        self.0.validate()
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.0.path()
    }

    pub(crate) fn package(&self) -> &StagedRpm {
        self.0.package()
    }

    pub(crate) fn stage(&self) -> &DnfStageId {
        self.0.stage()
    }

    pub(super) fn binding_value(&self) -> String {
        format!("signature-verified:{}", self.0.binding_value())
    }
}

fn valid_rpm_basename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.ends_with(".rpm")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_paths_are_bound_to_a_fresh_fixed_run_directory() {
        let first = DnfStageId::new();
        let second = DnfStageId::new();

        assert_ne!(first, second);
        first.validate().unwrap();
        assert_eq!(
            first.root_path().parent().unwrap(),
            std::path::Path::new("/run")
        );
        assert_eq!(first.packages_path().file_name().unwrap(), "packages");
        assert_eq!(first.rpmdb_path().file_name().unwrap(), "rpmdb");
        assert_eq!(first.cache_path().file_name().unwrap(), "cache");
    }

    #[test]
    fn staged_rpm_names_reject_paths_and_option_injection() {
        let stage = DnfStageId::new();

        assert!(stage.package("docker-ce-1.2-3.x86_64.rpm").is_ok());
        for invalid in [
            "../payload.rpm",
            "-bad.rpm",
            "subdir/file.rpm",
            "bad rpm.rpm",
        ] {
            assert!(stage.package(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn hash_bound_staged_rpm_requires_sha256_hex_and_binding_is_distinct_from_signature() {
        let stage = DnfStageId::new();
        let package = stage.package("docker-ce-1.2-3.x86_64.rpm").unwrap();
        let digest = "a".repeat(64);
        let hash_bound = HashBoundDnfRpm::new(package, digest.clone()).unwrap();
        let verified = VerifiedDnfRpm::after_rpmkeys_success(hash_bound.clone());

        assert!(hash_bound.binding_value().ends_with(&digest));
        assert!(verified.binding_value().contains("signature-verified"));
        assert!(
            HashBoundDnfRpm::new(
                stage.package("docker-ce-1.2-3.x86_64.rpm").unwrap(),
                "not-a-digest".into()
            )
            .is_err()
        );
    }
}
