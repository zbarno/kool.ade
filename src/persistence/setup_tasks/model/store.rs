use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::execution::docker::images::RuntimeProfile;

use super::{FILE_SCHEMA_VERSION, SetupTaskV1};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetupTaskStoreV1 {
    pub schema_version: u8,
    pub latest_scan_generation: u64,
    pub tasks: BTreeMap<String, SetupTaskV1>,
    /// Profiles requested by actual workloads; the next scan binds them to
    /// the current trusted engine identity and architecture.
    #[serde(default)]
    pub required_image_profiles: BTreeSet<RuntimeProfile>,
}

pub type Store = SetupTaskStoreV1;

impl Default for SetupTaskStoreV1 {
    fn default() -> Self {
        Self {
            schema_version: FILE_SCHEMA_VERSION,
            latest_scan_generation: 0,
            tasks: BTreeMap::new(),
            required_image_profiles: BTreeSet::new(),
        }
    }
}
