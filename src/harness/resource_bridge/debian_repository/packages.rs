mod dependencies;
mod index;
mod plan;

#[cfg(test)]
mod tests;

pub(crate) use index::{InstalledPackage, PackageIndex, PackageRecord};
pub(crate) use plan::RepositoryPackagePlan;

#[cfg(test)]
use index::parse_record;
