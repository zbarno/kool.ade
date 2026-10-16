use super::{InstalledPackage, PackageIndex, PackageRecord};
use crate::execution::docker::image_extension::builder::{
    PackageArtifactIdentity, VerifiedPackageArtifact,
};
#[derive(Clone, Debug)]
pub(crate) struct RepositoryPackagePlan {
    records: Vec<PackageRecord>,
}

impl RepositoryPackagePlan {
    /// Resolve signed repository metadata before obtaining package approval.
    pub(crate) fn resolve(
        requested: &[String],
        installed_base: &std::collections::BTreeMap<String, InstalledPackage>,
        architecture: &str,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            architecture == "amd64" || architecture == "arm64",
            "unsupported Debian target architecture"
        );
        let release_bytes = crate::harness::resource_bridge::fetch::retrieve_debian_artifact(
            "dists/bookworm/InRelease",
            16 * 1024 * 1024,
        )?;
        let release = super::super::verify_bookworm_inrelease(&release_bytes)?;
        let index_path = format!("main/binary-{architecture}/Packages.gz");
        anyhow::ensure!(
            release.sha256.contains_key(&index_path),
            "signed Debian Release does not cover the requested architecture index"
        );
        let index_bytes = crate::harness::resource_bridge::fetch::retrieve_debian_artifact(
            &format!("dists/bookworm/{index_path}"),
            128 * 1024 * 1024,
        )?;
        let index = PackageIndex::parse_gzip(&release, &index_path, &index_bytes)?;
        Ok(Self {
            records: index.closure(requested, installed_base, architecture)?,
        })
    }

    pub(crate) fn identities(&self) -> Vec<PackageArtifactIdentity> {
        self.records
            .iter()
            .map(|record| PackageArtifactIdentity {
                name: record.name.clone(),
                version: record.version.clone(),
                architecture: record.architecture.clone(),
                source: record.source_url(),
                sha256: record.sha256.clone(),
            })
            .collect()
    }

    /// Acquire package bytes only after authorization binds the exact closure.
    pub(crate) fn fetch_authorized(
        &self,
        authorized: &[PackageArtifactIdentity],
    ) -> anyhow::Result<Vec<VerifiedPackageArtifact>> {
        anyhow::ensure!(
            self.identities() == authorized,
            "approved package manifest differs from signed repository plan"
        );
        let total_size = self
            .records
            .iter()
            .try_fold(0_u64, |total, record| total.checked_add(record.size))
            .ok_or_else(|| anyhow::anyhow!("Debian closure size overflow"))?;
        anyhow::ensure!(
            total_size <= 512 * 1024 * 1024,
            "Debian package closure exceeds the aggregate transfer limit"
        );
        self.records
            .iter()
            .map(|record| {
                let bytes = crate::harness::resource_bridge::fetch::retrieve_debian_artifact(
                    &record.filename,
                    record.size,
                )?;
                record.verify_artifact(bytes)
            })
            .collect()
    }
}
