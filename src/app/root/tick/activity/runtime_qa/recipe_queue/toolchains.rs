use std::collections::BTreeSet;

use crate::execution::docker::images::{ImageLock, RuntimeProfile};

const PROFILES: [RuntimeProfile; 5] = [
    RuntimeProfile::General,
    RuntimeProfile::Rust,
    RuntimeProfile::Node,
    RuntimeProfile::Python,
    RuntimeProfile::Go,
];

pub(super) fn missing_toolchains(lock: &ImageLock, required_tools: &[String]) -> Vec<String> {
    let mut available = BTreeSet::new();
    for profile in PROFILES {
        for architecture in ["amd64", "arm64"] {
            if let Ok(image) = lock.select(profile, architecture) {
                available.extend(
                    image
                        .tool_versions
                        .keys()
                        .filter(|tool| !matches!(tool.as_str(), "base" | "shell"))
                        .cloned(),
                );
            }
        }
    }
    required_tools
        .iter()
        .filter(|tool| !available.contains(tool.as_str()))
        .cloned()
        .collect()
}

pub(super) fn has_compatible_image(lock: &ImageLock, required_tools: &[String]) -> bool {
    ["amd64", "arm64"].into_iter().any(|architecture| {
        lock.select_with_tools(&PROFILES, architecture, required_tools)
            .is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_only_tools_missing_from_all_approved_build_profiles() {
        let lock = ImageLock::bundled().unwrap();
        assert_eq!(
            missing_toolchains(&lock, &["npm".into(), "odin".into()]),
            ["odin"]
        );
        assert!(has_compatible_image(&lock, &["npm".into()]));
        assert!(!has_compatible_image(&lock, &["odin".into()]));
    }
}
