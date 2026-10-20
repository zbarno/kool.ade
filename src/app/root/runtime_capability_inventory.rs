use std::collections::BTreeSet;

use crate::{
    execution::docker::images::{ImageLock, RuntimeProfile},
    project_runtime::ProjectCapabilityManifestV1,
    project_runtime_capability::{ApprovedCapabilityInventory, ManifestTargetAssessment},
};

pub(crate) struct RuntimeCapabilityInventory {
    available_tool_ids: BTreeSet<String>,
    available_driver_ids: BTreeSet<String>,
}

impl RuntimeCapabilityInventory {
    pub(crate) fn from_locked_images(lock: &ImageLock) -> Self {
        let profiles = [
            RuntimeProfile::General,
            RuntimeProfile::Rust,
            RuntimeProfile::Node,
            RuntimeProfile::Python,
            RuntimeProfile::Go,
        ];
        let mut available_tool_ids = BTreeSet::new();
        for profile in profiles {
            for architecture in ["amd64", "arm64"] {
                if let Ok(image) = lock.select(profile, architecture) {
                    available_tool_ids.extend(
                        image
                            .tool_versions
                            .into_keys()
                            .filter(|tool| !matches!(tool.as_str(), "base" | "shell")),
                    );
                }
            }
        }
        let available_driver_ids = [
            ("web-driver", RuntimeProfile::BrowserQa),
            ("linux-desktop-driver", RuntimeProfile::DesktopQa),
        ]
        .into_iter()
        .filter(|(_, profile)| {
            ["amd64", "arm64"]
                .into_iter()
                .any(|architecture| lock.select(*profile, architecture).is_ok())
        })
        .map(|(id, _)| id.to_owned())
        .collect();
        Self {
            available_tool_ids,
            available_driver_ids,
        }
    }

    pub(crate) fn assess_target(
        &self,
        manifest: &ProjectCapabilityManifestV1,
        target_id: &str,
        task_id: &str,
    ) -> anyhow::Result<ManifestTargetAssessment> {
        let inventory = ApprovedCapabilityInventory {
            manifest,
            available_tool_ids: &self.available_tool_ids,
            // Broker-backed availability is not implemented yet, so do not
            // report project-specific dependencies as ready.
            available_dependency_ids: &BTreeSet::new(),
            available_driver_ids: &self.available_driver_ids,
        };
        crate::project_runtime_capability::assess_manifest_target(
            manifest, target_id, task_id, &inventory,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_uses_only_locked_image_tools_and_enabled_drivers() {
        let lock = ImageLock::bundled().unwrap();
        let inventory = RuntimeCapabilityInventory::from_locked_images(&lock);
        let mut expected_tools = BTreeSet::new();
        for profile in [
            RuntimeProfile::General,
            RuntimeProfile::Rust,
            RuntimeProfile::Node,
            RuntimeProfile::Python,
            RuntimeProfile::Go,
        ] {
            for architecture in ["amd64", "arm64"] {
                if let Ok(image) = lock.select(profile, architecture) {
                    expected_tools.extend(
                        image
                            .tool_versions
                            .into_keys()
                            .filter(|tool| !matches!(tool.as_str(), "base" | "shell")),
                    );
                }
            }
        }
        assert_eq!(inventory.available_tool_ids, expected_tools);
        assert!(!inventory.available_tool_ids.contains("base"));
        assert!(!inventory.available_tool_ids.contains("shell"));
        for (driver, profile) in [
            ("web-driver", RuntimeProfile::BrowserQa),
            ("linux-desktop-driver", RuntimeProfile::DesktopQa),
        ] {
            let expected = ["amd64", "arm64"]
                .into_iter()
                .any(|architecture| lock.select(profile, architecture).is_ok());
            assert_eq!(inventory.available_driver_ids.contains(driver), expected);
        }
    }
}
