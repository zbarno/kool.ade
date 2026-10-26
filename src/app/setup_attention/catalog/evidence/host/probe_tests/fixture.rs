use std::collections::HashMap;

use crate::app::setup_attention::catalog::ImageArchitectureV1 as Architecture;

use super::super::{EntryKind, EntryMetadata, ProbeRead, ProbeSource, inspect_linux};

pub(super) const UID: u32 = 1000;

#[derive(Default)]
pub(super) struct FixtureSource {
    pub text: HashMap<String, ProbeRead<String>>,
    pub metadata: HashMap<String, ProbeRead<EntryMetadata>>,
    pub executables: HashMap<String, ProbeRead<bool>>,
}

impl FixtureSource {
    pub(super) fn ready() -> Self {
        let mut source = Self::default();
        source.text("/etc/os-release", "ID=ubuntu\nVERSION_ID=24.04\n");
        source.text("/proc/sys/kernel/osrelease", "6.8.0-35-generic\n");
        source.text("/proc/sys/user/max_user_namespaces", "28633\n");
        source.text("/etc/subuid", "developer:100000:65536\n");
        source.text("/etc/subgid", "developer:200000:65536\n");
        source.text("/sys/fs/cgroup/cgroup.controllers", "cpu memory pids\n");
        source.text(
            "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/cgroup.controllers",
            "cpu memory pids\n",
        );
        source.text(
            "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/cgroup.subtree_control",
            "cpu memory pids\n",
        );
        source.set_metadata("/run/user/1000", EntryKind::Directory, UID, 0o700, false);
        source.set_metadata(
            "/run/user/1000/systemd/private",
            EntryKind::Socket,
            UID,
            0o600,
            false,
        );
        source.set_metadata("/run/user/1000/bus", EntryKind::Socket, UID, 0o600, false);
        for name in [
            "apt-get",
            "newuidmap",
            "newgidmap",
            "rootlesskit",
            "dockerd-rootless.sh",
            "dockerd-rootless-setuptool.sh",
        ] {
            source.set_executable(name, true);
        }
        source
    }

    pub(super) fn text(&mut self, path: &str, value: &str) {
        self.text
            .insert(path.into(), ProbeRead::Found(value.into()));
    }

    pub(super) fn set_metadata(
        &mut self,
        path: &str,
        kind: EntryKind,
        owner_uid: u32,
        mode: u32,
        symlink: bool,
    ) {
        self.metadata.insert(
            path.into(),
            ProbeRead::Found(EntryMetadata {
                kind,
                owner_uid,
                mode,
                symlink,
            }),
        );
    }

    pub(super) fn set_executable(&mut self, name: &str, found: bool) {
        self.executables
            .insert(name.into(), ProbeRead::Found(found));
    }
}

impl ProbeSource for FixtureSource {
    fn read_text(&self, path: &str) -> ProbeRead<String> {
        self.text.get(path).cloned().unwrap_or(ProbeRead::Missing)
    }

    fn metadata(&self, path: &str) -> ProbeRead<EntryMetadata> {
        self.metadata
            .get(path)
            .copied()
            .unwrap_or(ProbeRead::Missing)
    }

    fn executable(&self, name: &str) -> ProbeRead<bool> {
        self.executables
            .get(name)
            .copied()
            .unwrap_or(ProbeRead::Missing)
    }
}

pub(super) fn inspect(source: &FixtureSource) -> super::super::LinuxProbeFacts {
    inspect_linux(source, UID, Some("developer".into()), Architecture::Amd64)
}
