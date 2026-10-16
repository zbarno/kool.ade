use super::{InstalledPackage, PackageIndex, PackageRecord};
use crate::harness::resource_bridge::debian_repository::packages::dependencies::{
    DependencyAlternative, matches_target_arch, satisfies_version,
};
use std::collections::{BTreeMap, BTreeSet};

impl PackageIndex {
    /// Resolve Debian dependency groups using the signed candidate index and
    /// packages already present in the exact base image.
    pub(crate) fn closure(
        &self,
        requested: &[String],
        installed_base: &BTreeMap<String, InstalledPackage>,
        architecture: &str,
    ) -> anyhow::Result<Vec<PackageRecord>> {
        let mut pending = requested.iter().cloned().collect::<BTreeSet<_>>();
        let mut selected = BTreeMap::<String, PackageRecord>::new();
        while let Some(name) = pending.pop_first() {
            if selected.contains_key(&name) {
                continue;
            }
            let record = self
                .records
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("approved Debian index has no package {name}"))?
                .clone();
            anyhow::ensure!(
                record.size <= 512 * 1024 * 1024,
                "resolved Debian package exceeds the per-artifact transfer limit"
            );
            anyhow::ensure!(
                record.architecture == architecture || record.architecture == "all",
                "Debian package architecture does not match target"
            );
            for group in &record.dependencies {
                let resolved = group
                    .alternatives
                    .iter()
                    .find_map(|alternative| {
                        if !matches_target_arch(alternative, architecture) {
                            return None;
                        }
                        if installed_base
                            .get(&alternative.name)
                            .is_some_and(|installed| {
                                compatible_architecture(
                                    alternative,
                                    &installed.architecture,
                                    installed.multi_arch.as_deref(),
                                    architecture,
                                ) && satisfies_version(
                                    &installed.version,
                                    alternative.constraint.as_ref(),
                                )
                            })
                        {
                            return Some(None);
                        }
                        if let Some(candidate) =
                            self.records.get(&alternative.name).filter(|candidate| {
                                compatible_architecture(
                                    alternative,
                                    &candidate.architecture,
                                    candidate.multi_arch.as_deref(),
                                    architecture,
                                ) && satisfies_version(
                                    &candidate.version,
                                    alternative.constraint.as_ref(),
                                )
                            })
                        {
                            return Some(Some(candidate.name.clone()));
                        }
                        self.records
                            .values()
                            .find(|candidate| {
                                compatible_architecture(
                                    alternative,
                                    &candidate.architecture,
                                    candidate.multi_arch.as_deref(),
                                    architecture,
                                ) && candidate.provides.iter().any(|provided| {
                                    provided.name == alternative.name
                                        && alternative.constraint.as_ref().is_none_or(
                                            |constraint| {
                                                provided.version.as_deref().is_some_and(|version| {
                                                    satisfies_version(version, Some(constraint))
                                                })
                                            },
                                        )
                                })
                            })
                            .map(|candidate| Some(candidate.name.clone()))
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "no approved repository candidate satisfies a Debian dependency group"
                        )
                    })?;
                if let Some(package_name) = resolved {
                    pending.insert(package_name);
                }
            }
            selected.insert(name, record);
            anyhow::ensure!(
                selected.len() <= 256,
                "Debian dependency closure exceeds package limit"
            );
        }
        Ok(selected.into_values().collect())
    }
}

fn compatible_architecture(
    alternative: &DependencyAlternative,
    candidate_architecture: &str,
    multi_arch: Option<&str>,
    target_architecture: &str,
) -> bool {
    if candidate_architecture == "all" {
        return true;
    }
    if candidate_architecture != target_architecture {
        return false;
    }
    match alternative.qualifier.as_deref() {
        Some("any") => multi_arch == Some("allowed"),
        Some("native") | None => true,
        Some(_) => false,
    }
}
