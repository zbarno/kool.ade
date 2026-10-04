use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

use anyhow::Context;

use super::path_safety::{
    SENSITIVE_HOST_PATH_COMPONENTS, canonical_path_with_missing_tail, ensure_narrow_host_directory,
};

const DEDICATED_NUGET_DIRECTORY_NAMES: [&str; 4] = [
    "packages",
    "global-packages",
    "nuget-packages",
    "nuget_packages",
];
const TEMPORARY_HOST_ROOTS: [&str; 3] = ["/tmp", "/var/tmp", "/dev/shm"];
pub(in crate::harness::pi_sandbox) fn host_nuget_packages() -> anyhow::Result<PathBuf> {
    let configured = env::var_os("NUGET_PACKAGES").map(PathBuf::from);
    let home = env::var_os("HOME").map(PathBuf::from);
    prepare_nuget_packages(configured, home, &env::current_dir()?)
}

pub(super) fn prepare_nuget_packages(
    configured: Option<PathBuf>,
    home: Option<PathBuf>,
    current_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let source = if let Some(path) = configured.filter(|path| !path.as_os_str().is_empty()) {
        if path.is_absolute() {
            path
        } else {
            current_dir.join(path)
        }
    } else {
        let home = home
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot locate the host NuGet cache without HOME"))?;
        home.join(".nuget/packages")
    };

    let candidate = canonical_path_with_missing_tail(&source)?;
    let home = home
        .as_deref()
        .map(canonical_path_with_missing_tail)
        .transpose()?;
    validate_nuget_packages(&candidate, home.as_deref())?;
    fs::create_dir_all(&candidate).with_context(|| {
        format!(
            "Cannot prepare host NuGet package cache at {}",
            candidate.display()
        )
    })?;
    let source = source.canonicalize()?;
    validate_nuget_packages(&source, home.as_deref())?;
    Ok(source)
}

fn validate_nuget_packages(path: &Path, home: Option<&Path>) -> anyhow::Result<()> {
    ensure_narrow_host_directory(path, "NUGET_PACKAGES")?;
    if let Some(home) = home {
        anyhow::ensure!(
            path != home && !home.starts_with(path),
            "NUGET_PACKAGES cannot expose the host home directory or one of its parents"
        );
    }
    let directory_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    anyhow::ensure!(
        DEDICATED_NUGET_DIRECTORY_NAMES.contains(&directory_name.as_str()),
        "NUGET_PACKAGES must identify a dedicated NuGet package-cache directory"
    );
    anyhow::ensure!(
        !path
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .any(|name| {
                SENSITIVE_HOST_PATH_COMPONENTS
                    .iter()
                    .any(|blocked| name.eq_ignore_ascii_case(blocked))
            }),
        "NUGET_PACKAGES cannot be inside a sensitive host directory"
    );
    let temporary_root = env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| env::temp_dir());
    if path.starts_with(&temporary_root)
        || TEMPORARY_HOST_ROOTS
            .iter()
            .any(|root| path.starts_with(root))
    {
        anyhow::ensure!(
            home.is_some_and(|home| path.starts_with(home) && path != home),
            "NUGET_PACKAGES cannot be in a temporary directory outside the host home"
        );
    }
    anyhow::ensure!(
        !contains_nuget_config(path),
        "NUGET_PACKAGES cannot expose a NuGet configuration file"
    );
    Ok(())
}

fn contains_nuget_config(path: &Path) -> bool {
    [
        "NuGet.Config",
        "nuget.config",
        "NuGet/NuGet.Config",
        "nuget/nuget.config",
    ]
    .iter()
    .any(|name| path.join(name).is_file())
}
