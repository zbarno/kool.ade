//! Narrow host-side refresh for NuGet's public vulnerability feed.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const AUDIT_URL: &str = "https://api.nuget.org/v3/index.json";

pub(super) fn cache_path() -> anyhow::Result<PathBuf> {
    let state_root = crate::persistence::state_root();
    fs::create_dir_all(&state_root)?;
    let state_root = state_root.canonicalize()?;
    let mut path = state_root.clone();
    for component in ["cache", "nuget-audit-http"] {
        path.push(component);
        ensure_private_directory(&path)?;
        anyhow::ensure!(
            path.canonicalize()?.starts_with(&state_root),
            "NuGet audit cache escapes the Kool.ad/e state directory"
        );
    }
    Ok(path)
}

fn ensure_private_directory(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "NuGet audit cache path contains a symlink or non-directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)?,
        Err(error) => return Err(error.into()),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(super) fn refresh(timeout: Duration) -> anyhow::Result<()> {
    let root = unique_temp_dir()?;
    let result = refresh_in(&root, timeout);
    let _ = fs::remove_dir_all(&root);
    result
}

fn refresh_in(root: &Path, timeout: Duration) -> anyhow::Result<()> {
    let home = root.join("home");
    let packages = root.join("packages");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&packages)?;
    let project = root.join("audit-refresh.csproj");
    let config = root.join("NuGet.Config");
    let dotnet = find_dotnet()?;
    let sdk = Command::new(&dotnet)
        .env_clear()
        .env("PATH", env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("DOTNET_CLI_HOME", &home)
        .env("DOTNET_NOLOGO", "1")
        .arg("--version")
        .output()?;
    anyhow::ensure!(sdk.status.success(), "Cannot inspect the host .NET SDK");
    let version = String::from_utf8_lossy(&sdk.stdout);
    let major = version
        .trim()
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .ok_or_else(|| anyhow::anyhow!("Cannot determine host .NET SDK version"))?;
    let framework = if major >= 5 {
        format!("net{major}.0")
    } else {
        format!("netcoreapp{major}.0")
    };
    fs::write(
        &project,
        format!(
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><TargetFramework>{framework}</TargetFramework><NuGetAudit>true</NuGetAudit><NuGetAuditMode>all</NuGetAuditMode></PropertyGroup>
  <ItemGroup><PackageReference Include="Newtonsoft.Json" Version="13.0.3" /></ItemGroup>
</Project>
"#
        ),
    )?;
    fs::write(
        &config,
        format!(
            "<configuration><packageSources><clear /><add key=\"nuget.org\" value=\"{AUDIT_URL}\" /></packageSources><auditSources><clear /><add key=\"nuget.org\" value=\"{AUDIT_URL}\" /></auditSources></configuration>"
        ),
    )?;

    let stdout_path = root.join("restore.stdout");
    let stderr_path = root.join("restore.stderr");
    let stdout = fs::File::create(&stdout_path)?;
    let stderr = fs::File::create(&stderr_path)?;
    let mut child = Command::new(dotnet)
        .env_clear()
        .env("PATH", env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("DOTNET_CLI_HOME", &home)
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .env("NUGET_PACKAGES", &packages)
        .env("NUGET_HTTP_CACHE_PATH", cache_path()?)
        .args([
            "restore",
            project
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Temporary project path is not UTF-8"))?,
            "--configfile",
            config
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Temporary config path is not UTF-8"))?,
            "--verbosity",
            "quiet",
        ])
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!(
                "Timed out refreshing the public NuGet audit feed after {} seconds",
                timeout.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(100));
    };
    let stdout = fs::read_to_string(stdout_path)?;
    let detail = fs::read_to_string(stderr_path)?;
    let logs = format!("{stdout}\n{detail}");
    anyhow::ensure!(
        status.success() && !logs.to_ascii_lowercase().contains("nu1900"),
        "NuGet public audit refresh failed: {}",
        logs.trim()
    );
    anyhow::ensure!(
        cache_contains_audit_data(&cache_path()?),
        "NuGet restore completed without caching public vulnerability data"
    );
    Ok(())
}

fn cache_contains_audit_data(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_type().is_ok_and(|kind| kind.is_dir())
            && fs::read_dir(entry.path()).is_ok_and(|children| {
                children.flatten().any(|child| {
                    child.file_name().to_string_lossy().starts_with("vuln_")
                        && child.metadata().is_ok_and(|metadata| metadata.len() > 0)
                })
            })
    })
}

fn find_dotnet() -> anyhow::Result<PathBuf> {
    if let Some(path) = crate::harness::pi_sandbox::host_dotnet_executable() {
        return Ok(path);
    }
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("dotnet"))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow::anyhow!("Host .NET SDK is unavailable for the managed NuGet audit refresh")
        })
}

fn unique_temp_dir() -> anyhow::Result<PathBuf> {
    let path = env::temp_dir().join(format!(
        "koolade-nuget-audit-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_nonempty_nuget_vulnerability_entries_count_as_audit_cache_data() {
        let root = unique_temp_dir().unwrap();
        let bucket = root.join("source-cache");
        fs::create_dir(&bucket).unwrap();
        assert!(!cache_contains_audit_data(&root));
        fs::write(bucket.join("vuln_index.dat"), "cached audit index").unwrap();
        assert!(cache_contains_audit_data(&root));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cache_directory_validation_rejects_symlinks() {
        let root = unique_temp_dir().unwrap();
        let target = root.join("target");
        let link = root.join("cache-link");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(ensure_private_directory(&link).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
