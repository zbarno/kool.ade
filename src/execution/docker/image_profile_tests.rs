use super::images::{ImageLock, ImageVariant, RuntimeProfile};

#[test]
fn bundled_language_profiles_select_the_locked_version_for_each_architecture() {
    let lock = ImageLock::bundled().unwrap();
    for (profile, repo, tool, version, amd64, arm64) in [
        (
            RuntimeProfile::Python,
            "docker.io/library/python",
            "python",
            "3.14.7",
            "sha256:998acd06f485adfd6890e3e15a4b542543e0cf22ff904310095da328a5e3e561",
            "sha256:af1cafd27effb3b1c7c4ad8b459f118371b5d216d4659a3e68a7b5aae65b517f",
        ),
        (
            RuntimeProfile::Go,
            "docker.io/library/golang",
            "go",
            "1.26.8",
            "sha256:26de5cf8eac40c12a1ff8de1a634971b4654fd881225eed7ace643b8c769f2f5",
            "sha256:581f0292073059289b736d46d572ba4c515e0b511f8645963e0b0adf232680a1",
        ),
    ] {
        for (arch, digest) in [("amd64", amd64), ("arm64", arm64)] {
            let selected = lock.select(profile, arch).unwrap();
            assert_eq!(selected.platform, format!("linux/{arch}"));
            assert_eq!(selected.digest, digest);
            assert_eq!(selected.reference, format!("{repo}@{digest}"));
            assert_eq!(
                selected.tool_versions.get(tool).map(String::as_str),
                Some(version)
            );
            assert_eq!(selected.runtime_user, "10001:10001");
        }
    }
}

#[test]
fn fake_runtime_profiles_are_selected_only_when_all_recipe_tools_match() {
    let mut lock = ImageLock::bundled().unwrap();
    for (profile, tool, version, repo) in [
        ("python", "python", "3.14.7", "docker.io/library/python"),
        ("go", "go", "1.26.8", "docker.io/library/golang"),
    ] {
        for arch in ["linux/amd64", "linux/arm64"] {
            let fake_digest = format!(
                "sha256:{}",
                if arch.ends_with("amd64") { "a" } else { "b" }.repeat(64)
            );
            let selected = lock
                .profiles
                .get_mut(profile)
                .unwrap()
                .iter_mut()
                .find(|variant| variant.platform == arch)
                .unwrap();
            selected.reference = format!("{repo}@{fake_digest}");
            selected.digest = fake_digest;
            selected.tool_versions.insert(tool.into(), version.into());
        }
    }
    lock.validate().unwrap();
    for (profile, tool, version) in [
        (RuntimeProfile::Python, "python", "3.14.7"),
        (RuntimeProfile::Go, "go", "1.26.8"),
    ] {
        let selection = lock
            .select_with_tools(&[profile], "arm64", &[tool.into()])
            .unwrap();
        assert_eq!(selection.profile, profile);
        assert_eq!(selection.platform, "linux/arm64");
        assert_eq!(
            selection.tool_versions.get(tool).map(String::as_str),
            Some(version)
        );
        assert!(
            lock.select_with_tools(&[profile], "arm64", &["missing-tool".into()])
                .is_err()
        );
    }
}

#[test]
fn new_profile_versions_are_fixed_by_validation() {
    let mut lock = ImageLock::bundled().unwrap();
    let python = lock.profiles.get_mut("python").unwrap();
    python[0]
        .tool_versions
        .insert("python".into(), "3.14.8".into());
    assert!(lock.validate().is_err());
    let mut lock = ImageLock::bundled().unwrap();
    let go: &mut Vec<ImageVariant> = lock.profiles.get_mut("go").unwrap();
    go[0].tool_versions.insert("go".into(), "1.26.7".into());
    assert!(lock.validate().is_err());
}
