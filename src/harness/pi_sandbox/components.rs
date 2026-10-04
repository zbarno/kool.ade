//! Narrow read-only mounts for host toolchains and durable package caches.
use std::{collections::BTreeSet, path::PathBuf};

use super::mounts::{bind_readonly, push_env};

mod dotnet;
mod nuget;
mod path_safety;

use dotnet::{host_dotnet_root, mount_dotnet_root};
pub(super) use nuget::host_nuget_packages;
#[cfg(test)]
use nuget::prepare_nuget_packages;

const SANDBOX_DOTNET_ROOT: &str = "/tmp/koolade-tools/dotnet";
const SANDBOX_NUGET_PACKAGES: &str = "/tmp/koolade-home/.nuget/packages";

pub(super) struct RuntimeComponents {
    dotnet_root: Option<PathBuf>,
    nuget_packages: PathBuf,
}

impl RuntimeComponents {
    pub(super) fn mount(
        args: &mut Vec<String>,
        created: &mut BTreeSet<String>,
    ) -> anyhow::Result<Self> {
        let dotnet_root = host_dotnet_root()
            .map(|root| mount_dotnet_root(args, created, &root))
            .transpose()?;
        let host_packages = host_nuget_packages()?;
        let nuget_packages = PathBuf::from(SANDBOX_NUGET_PACKAGES);
        bind_readonly(args, created, &host_packages, &nuget_packages)?;
        Ok(Self {
            dotnet_root,
            nuget_packages,
        })
    }

    pub(super) fn set_environment(&self, args: &mut Vec<String>, base_path: &str) {
        let path = self
            .dotnet_root
            .as_ref()
            .map(|root| format!("{}:{base_path}", root.display()))
            .unwrap_or_else(|| base_path.to_owned());
        push_env(args, "PATH", &path);
        push_env(
            args,
            "NUGET_PACKAGES",
            &self.nuget_packages.to_string_lossy(),
        );
        if let Some(root) = &self.dotnet_root {
            push_env(args, "DOTNET_ROOT", &root.to_string_lossy());
        }
    }
}

#[cfg(test)]
mod tests;
