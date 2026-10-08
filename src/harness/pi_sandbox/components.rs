//! Narrow read-only mounts for host toolchains and durable package caches.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use super::mounts::{bind_readonly, push_env};

mod dotnet;
mod node;
#[cfg(test)]
mod npm;
mod nuget;
mod path_safety;

use dotnet::{host_dotnet_root, mount_dotnet_root};
pub(super) use node::host_node_root;
use node::{add_node_path, mount_node_runtime};
pub(super) use nuget::host_nuget_packages;
#[cfg(test)]
use nuget::prepare_nuget_packages;

const SANDBOX_DOTNET_ROOT: &str = "/tmp/koolade-tools/dotnet";
const SANDBOX_NUGET_PACKAGES: &str = "/tmp/koolade-home/.nuget/packages";
const SANDBOX_NUGET_HTTP_CACHE: &str = "/tmp/koolade-home/.nuget/http-cache";

pub(super) fn host_dotnet_executable() -> Option<PathBuf> {
    host_dotnet_root().map(|root| root.join("dotnet"))
}

pub(super) struct RuntimeComponents {
    dotnet_root: Option<PathBuf>,
    node_root: Option<PathBuf>,
    nuget_packages: PathBuf,
    nuget_http_cache: PathBuf,
}

impl RuntimeComponents {
    pub(super) fn mount(
        args: &mut Vec<String>,
        created: &mut BTreeSet<String>,
        empty_file: &Path,
    ) -> anyhow::Result<Self> {
        let dotnet_root = host_dotnet_root()
            .map(|root| mount_dotnet_root(args, created, &root))
            .transpose()?;
        let node_root = mount_node_runtime(args, created, empty_file)?;
        let host_packages = host_nuget_packages()?;
        let nuget_packages = PathBuf::from(SANDBOX_NUGET_PACKAGES);
        bind_readonly(args, created, &host_packages, &nuget_packages)?;
        let nuget_http_cache = crate::harness::nuget_audit_cache_path()?;
        let sandbox_nuget_http_cache = PathBuf::from(SANDBOX_NUGET_HTTP_CACHE);
        bind_readonly(args, created, &nuget_http_cache, &sandbox_nuget_http_cache)?;
        Ok(Self {
            dotnet_root,
            node_root,
            nuget_packages,
            nuget_http_cache: sandbox_nuget_http_cache,
        })
    }

    pub(super) fn set_environment(&self, args: &mut Vec<String>, base_path: &str) {
        let path = self
            .dotnet_root
            .as_ref()
            .map(|root| format!("{}:{base_path}", root.display()))
            .unwrap_or_else(|| base_path.to_owned());
        let mut path = path;
        add_node_path(&mut path, self.node_root.as_deref());
        push_env(args, "PATH", &path);
        push_env(
            args,
            "NUGET_PACKAGES",
            &self.nuget_packages.to_string_lossy(),
        );
        push_env(
            args,
            "NUGET_HTTP_CACHE_PATH",
            &self.nuget_http_cache.to_string_lossy(),
        );
        push_env(args, "npm_config_offline", "true");
        push_env(args, "npm_config_audit", "false");
        push_env(
            args,
            "npm_config_globalconfig",
            "/tmp/koolade-home/.npm-globalrc",
        );
        push_env(args, "CARGO_NET_OFFLINE", "true");
        if let Some(root) = &self.dotnet_root {
            push_env(args, "DOTNET_ROOT", &root.to_string_lossy());
        }
    }
}

#[cfg(test)]
mod tests;
