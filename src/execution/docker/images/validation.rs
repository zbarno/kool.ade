use std::collections::BTreeSet;

use super::{
    ImageLock, PINNED_GO_RUNTIME, PINNED_PYTHON_RUNTIME, PINNED_RUST_TOOLCHAIN, failure, is_digest,
};
use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

impl ImageLock {
    pub fn validate(&self) -> Result<(), PrerequisiteFailure> {
        if self.schema_version != 1 || self.image_set_version.trim().is_empty() {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Executor image manifest version is unsupported",
                false,
            ));
        }
        let mut profiles = vec!["general", "rust", "node", "python", "go"];
        profiles.extend(
            ["browser_qa", "desktop_qa"]
                .into_iter()
                .filter(|profile| self.profiles.contains_key(*profile)),
        );
        for profile in profiles {
            let variants = self.profiles.get(profile).ok_or_else(|| {
                failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Executor image manifest is missing an approved profile",
                    false,
                )
            })?;
            let mut platforms = BTreeSet::new();
            let source_repository = match profile {
                "general" => "docker.io/library/debian",
                "rust" => "docker.io/library/rust",
                "node" => "docker.io/library/node",
                "python" => "docker.io/library/python",
                "go" => "docker.io/library/golang",
                "browser_qa" => super::TRUSTED_BROWSER_QA_REPOSITORY,
                "desktop_qa" => super::TRUSTED_DESKTOP_QA_REPOSITORY,
                _ => unreachable!(),
            };
            let required_tool_metadata: &[&str] = match profile {
                "general" => &["shell", "base"],
                "rust" => &["rustc", "cargo", "base"],
                "node" => &["node", "npm", "base"],
                "python" => &["python", "base"],
                "go" => &["go", "base"],
                "browser_qa" => &[
                    "node",
                    "npm",
                    "python3",
                    "playwright",
                    "chromium",
                    "driver",
                    "base",
                ],
                "desktop_qa" => &["driver", "xvfb", "base"],
                _ => unreachable!(),
            };
            for variant in variants {
                validate_variant(profile, source_repository, required_tool_metadata, variant)?;
                if !platforms.insert(variant.platform.as_str()) {
                    return Err(failure(
                        PrerequisiteCode::ImageUnavailable,
                        "Each approved executor profile must lock each platform exactly once",
                        false,
                    ));
                }
            }
            let expected: BTreeSet<_> = if matches!(profile, "browser_qa" | "desktop_qa") {
                ["linux/amd64"].into_iter().collect()
            } else {
                ["linux/amd64", "linux/arm64"].into_iter().collect()
            };
            if platforms != expected {
                return Err(failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Each approved executor profile must lock linux/amd64 and linux/arm64",
                    false,
                ));
            }
        }
        if self.profiles.keys().any(|profile| {
            ![
                "general",
                "rust",
                "node",
                "python",
                "go",
                "browser_qa",
                "desktop_qa",
            ]
            .contains(&profile.as_str())
        }) {
            return Err(failure(
                PrerequisiteCode::ProfileUnsupported,
                "Executor image manifest contains an unsupported profile",
                false,
            ));
        }
        Ok(())
    }
}

fn validate_variant(
    profile: &str,
    source_repository: &str,
    required_tool_metadata: &[&str],
    variant: &super::ImageVariant,
) -> Result<(), PrerequisiteFailure> {
    if !matches!(variant.platform.as_str(), "linux/amd64" | "linux/arm64")
        || !is_digest(&variant.digest)
        || variant.reference
            != format!(
                "{}@{}",
                variant.reference.split('@').next().unwrap_or(""),
                variant.digest
            )
        || !matches!(variant.reference.split('@').next(), Some(repository) if repository == source_repository)
        || (variant.reference.contains(':') && !variant.reference.contains("@sha256:"))
    {
        return Err(failure(
            PrerequisiteCode::ImageUnavailable,
            "Executor image manifest contains an invalid platform or non-immutable image reference",
            false,
        ));
    }
    if variant
        .tool_versions
        .values()
        .any(|version| version.trim().is_empty())
    {
        return Err(failure(
            PrerequisiteCode::ImageUnavailable,
            "Executor image manifest contains an empty tool version",
            false,
        ));
    }
    if matches!(profile, "browser_qa" | "desktop_qa")
        && variant
            .build_recipe_sha256
            .as_deref()
            .is_none_or(|digest| !is_digest(digest))
    {
        return Err(failure(
            PrerequisiteCode::ImageUnavailable,
            "QA runtime image is not a pinned, reviewed build",
            false,
        ));
    }
    if required_tool_metadata
        .iter()
        .any(|key| !variant.tool_versions.contains_key(*key))
    {
        return Err(failure(
            PrerequisiteCode::ImageUnavailable,
            "Executor image manifest is missing required profile tool metadata",
            false,
        ));
    }
    if profile == "desktop_qa"
        && variant.runtime_enabled
        && ["chromium", "dbus"]
            .iter()
            .any(|key| !variant.tool_versions.contains_key(*key))
    {
        return Err(failure(
            PrerequisiteCode::ImageUnavailable,
            "Enabled Linux desktop QA image is missing Chromium or D-Bus version metadata",
            false,
        ));
    }
    if (profile == "rust"
        && (variant.tool_versions.get("rustc").map(String::as_str) != Some(PINNED_RUST_TOOLCHAIN)
            || variant.tool_versions.get("cargo").map(String::as_str)
                != Some(PINNED_RUST_TOOLCHAIN)))
        || (profile == "python"
            && variant.tool_versions.get("python").map(String::as_str)
                != Some(PINNED_PYTHON_RUNTIME))
        || (profile == "go"
            && variant.tool_versions.get("go").map(String::as_str) != Some(PINNED_GO_RUNTIME))
    {
        return Err(failure(
            PrerequisiteCode::ImageUnavailable,
            "Executor image tool versions do not match their pinned runtimes",
            false,
        ));
    }
    Ok(())
}
