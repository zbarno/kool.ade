use std::collections::BTreeSet;

use crate::{
    execution::docker::images::{ImageLock, RuntimeProfile},
    project_runtime::{
        ProjectCapabilityManifestV1, RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1,
    },
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
        available_dependency_ids: &BTreeSet<String>,
    ) -> anyhow::Result<ManifestTargetAssessment> {
        let inventory = ApprovedCapabilityInventory {
            manifest,
            available_tool_ids: &self.available_tool_ids,
            available_dependency_ids,
            available_driver_ids: &self.available_driver_ids,
        };
        crate::project_runtime_capability::assess_manifest_target(
            manifest, target_id, task_id, &inventory,
        )
    }
}

pub(crate) fn prepared_dependency_ids_for_target(
    receipts: &[RuntimeReceiptV1],
    task_id: &str,
    target_id: &str,
    source_sha: &str,
    start_recipe_id: &str,
    required_dependency_ids: &[String],
) -> BTreeSet<String> {
    let Some(receipt) = receipts
        .iter()
        .filter(|receipt| {
            receipt.operation == RuntimeOperation::Start
                && receipt.task_id == task_id
                && receipt.target_id == target_id
                && receipt.source_sha == source_sha
                && receipt.recipe_id.as_deref() == Some(start_recipe_id)
        })
        .max_by_key(|receipt| (receipt.observed_at_unix_ms, receipt.receipt_id.clone()))
    else {
        return BTreeSet::new();
    };
    if receipt.state != RuntimeReceiptState::Passed || receipt.exit_code != Some(0) {
        return BTreeSet::new();
    }
    let required = required_dependency_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if receipt
        .prepared_dependency_ids
        .iter()
        .any(|dependency_id| !required.contains(dependency_id.as_str()))
    {
        return BTreeSet::new();
    }
    receipt.prepared_dependency_ids.iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_availability_requires_a_matching_passed_start_receipt() {
        let ids = vec!["npm:zod".to_owned()];
        let mut receipt = RuntimeReceiptV1 {
            schema_version: 1,
            receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            operation: RuntimeOperation::Start,
            task_id: "task-1".into(),
            target_id: "web".into(),
            source_sha: "a".repeat(40),
            recipe_id: Some("start-v1".into()),
            state: RuntimeReceiptState::Passed,
            exit_code: Some(0),
            artifact_hashes: Default::default(),
            prepared_dependency_ids: ids.clone(),
            log_reference: None,
            observed_at_unix_ms: 1,
        };
        let find = |receipts: &[RuntimeReceiptV1]| {
            prepared_dependency_ids_for_target(
                receipts,
                "task-1",
                "web",
                &"a".repeat(40),
                "start-v1",
                &ids,
            )
        };
        assert_eq!(
            find(std::slice::from_ref(&receipt)),
            ids.iter().cloned().collect()
        );

        receipt.state = RuntimeReceiptState::Failed;
        assert!(find(std::slice::from_ref(&receipt)).is_empty());
        receipt.state = RuntimeReceiptState::Passed;
        receipt.target_id = "service".into();
        assert!(find(std::slice::from_ref(&receipt)).is_empty());
        receipt.target_id = "web".into();
        receipt.prepared_dependency_ids.push("cargo:serde".into());
        assert!(find(std::slice::from_ref(&receipt)).is_empty());

        receipt.prepared_dependency_ids = ids.clone();
        let mut later_failure = receipt.clone();
        later_failure.receipt_id = uuid::Uuid::new_v4().hyphenated().to_string();
        later_failure.state = RuntimeReceiptState::Failed;
        later_failure.exit_code = Some(1);
        later_failure.observed_at_unix_ms = 2;
        assert!(find(&[receipt, later_failure]).is_empty());
    }

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
