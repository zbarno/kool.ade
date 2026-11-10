use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionPayloadV1, ComputerActionV1, ComputerOperationV1,
    apps::{GuestAppArchitectureV1, StagedTaskApplicationV1},
};

use super::super::protocol::ReplyV1;

mod validation;
use self::validation::*;

pub(super) fn consume_nonce(action: &ComputerActionV1, operation: &str) -> bool {
    validation::consume_helper_nonce(action, operation)
}

const STAGING_ROOT: &str = "/var/lib/koolade-app-helper/task-staging";
const MAX_STAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CHUNK_BYTES: usize = 128 * 1024;

pub(in crate::computer_use::apps::guest_helper) struct PendingTaskStage {
    action: ComputerActionV1,
    application: StagedTaskApplicationV1,
    root: PathBuf,
    files: Vec<File>,
    offsets: Vec<u64>,
    committed: bool,
}

impl PendingTaskStage {
    pub(super) fn begin(action: &ComputerActionV1) -> Result<Self, ReplyV1> {
        let application = match &action.payload {
            ComputerActionPayloadV1::LaunchApprovedStagedTaskApp { application } => application,
            _ => return Err(ReplyV1::Unsupported),
        };
        let config = super::catalog::helper_config().map_err(|_| ReplyV1::Unsupported)?;
        let now = unix_time_ms();
        if !valid_action_scope(
            action,
            application,
            &config,
            crate::computer_use::ComputerActionHelperPhaseV1::Stage,
            now,
        ) || application.validate_at(now).is_err()
            || !architecture_matches(application.guest_architecture)
            || !consume_helper_nonce(action, "stage")
        {
            return Err(ReplyV1::Unsupported);
        }
        let total = application.artifacts.iter().fold(0_u64, |sum, artifact| {
            sum.saturating_add(artifact.receipt.size_bytes)
        });
        if total == 0 || total > MAX_STAGE_BYTES {
            return Err(ReplyV1::Unsupported);
        }
        let session_root = Path::new(STAGING_ROOT).join(&config.session_id);
        if fs::symlink_metadata(&session_root).is_ok() {
            return Err(ReplyV1::Unsupported);
        }
        let root = session_root.join(format!(".{}.partial", action.idempotency_key));
        create_private_directory(&session_root).map_err(|_| ReplyV1::Failed)?;
        let result = (|| {
            create_private_directory(&root)?;
            let mut files = Vec::with_capacity(application.artifacts.len());
            for artifact in &application.artifacts {
                let path = root.join(&artifact.relative_path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut options = OpenOptions::new();
                options
                    .write(true)
                    .read(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
                files.push(options.open(path)?);
            }
            Ok::<_, std::io::Error>(files)
        })();
        let files = match result {
            Ok(files) => files,
            Err(_) => {
                let _ = fs::remove_dir_all(&root);
                let _ = fs::remove_dir(&session_root);
                return Err(ReplyV1::Failed);
            }
        };
        if !has_stage_space(Path::new(STAGING_ROOT), total) {
            let _ = fs::remove_dir_all(&root);
            let _ = fs::remove_dir(&session_root);
            return Err(ReplyV1::Failed);
        }
        Ok(Self {
            action: action.clone(),
            application: (**application).clone(),
            root,
            files,
            offsets: vec![0; application.artifacts.len()],
            committed: false,
        })
    }

    pub(super) fn write_chunk(
        &mut self,
        artifact_index: u16,
        offset: u64,
        bytes: &[u8],
    ) -> ReplyV1 {
        let index = artifact_index as usize;
        let Some(file) = self.files.get_mut(index) else {
            return ReplyV1::Failed;
        };
        let Some(expected) = self.application.artifacts.get(index) else {
            return ReplyV1::Failed;
        };
        if bytes.is_empty()
            || bytes.len() > MAX_CHUNK_BYTES
            || self.offsets[index] != offset
            || offset.saturating_add(bytes.len() as u64) > expected.receipt.size_bytes
        {
            return ReplyV1::Failed;
        }
        if file.write_all(bytes).is_err() {
            return ReplyV1::Failed;
        }
        self.offsets[index] = offset.saturating_add(bytes.len() as u64);
        ReplyV1::Completed
    }

    pub(super) fn finish(mut self) -> ReplyV1 {
        if self
            .offsets
            .iter()
            .zip(&self.application.artifacts)
            .any(|(offset, artifact)| *offset != artifact.receipt.size_bytes)
        {
            return ReplyV1::Failed;
        }
        for (index, file) in self.files.iter_mut().enumerate() {
            if file.sync_all().is_err() {
                return ReplyV1::Failed;
            }
            let artifact = &self.application.artifacts[index];
            let path = self.root.join(&artifact.relative_path);
            if !file_matches_receipt(&path, artifact.receipt.size_bytes, &artifact.receipt.sha256) {
                return ReplyV1::Failed;
            }
            let Ok(bytes) = read_bounded(&path, artifact.receipt.size_bytes) else {
                return ReplyV1::Failed;
            };
            if artifact.relative_path == self.application.executable_relative_path
                && !valid_elf_executable(&bytes, self.application.guest_architecture)
            {
                return ReplyV1::Unsupported;
            }
            let mode = if artifact.relative_path == self.application.executable_relative_path {
                0o555
            } else {
                0o444
            };
            if fs::set_permissions(&path, fs::Permissions::from_mode(mode)).is_err() {
                return ReplyV1::Failed;
            }
        }
        let manifest_path = self.root.join(".koolade-staged-app.json");
        let Ok(manifest_bytes) = serde_json::to_vec(&self.application) else {
            return ReplyV1::Failed;
        };
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o400)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let Ok(mut manifest) = options.open(manifest_path) else {
            return ReplyV1::Failed;
        };
        if manifest.write_all(&manifest_bytes).is_err() || manifest.sync_all().is_err() {
            return ReplyV1::Failed;
        }
        if persist_signed_action(&self.root, &self.action).is_err() {
            return ReplyV1::Failed;
        }
        if !set_tree_read_only(&self.root) {
            return ReplyV1::Failed;
        }
        let final_path = session_root_path(&self.action).join(&self.action.idempotency_key);
        if fs::rename(&self.root, &final_path).is_err() {
            return ReplyV1::Failed;
        }
        if fs::set_permissions(
            final_path
                .parent()
                .unwrap_or_else(|| Path::new(STAGING_ROOT)),
            fs::Permissions::from_mode(0o711),
        )
        .is_err()
        {
            return ReplyV1::Failed;
        }
        self.committed = true;
        ReplyV1::Completed
    }
}

impl Drop for PendingTaskStage {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.root);
            if let Some(parent) = self.root.parent() {
                let _ = fs::remove_dir(parent);
            }
        }
    }
}

pub(super) fn launch(action: &ComputerActionV1) -> ReplyV1 {
    let application = match &action.payload {
        ComputerActionPayloadV1::LaunchApprovedStagedTaskApp { application } => application,
        _ => return ReplyV1::Unsupported,
    };
    let Ok(config) = super::catalog::helper_config() else {
        return ReplyV1::Unsupported;
    };
    let now = unix_time_ms();
    if !valid_action_scope(
        action,
        application,
        &config,
        crate::computer_use::ComputerActionHelperPhaseV1::Launch,
        now,
    ) || application.validate_at(now).is_err()
        || !architecture_matches(application.guest_architecture)
    {
        return ReplyV1::Unsupported;
    }
    let session_root = session_root_path(action);
    let staged_root = session_root.join(&action.idempotency_key);
    let Some(authorization) = action.helper_authorization.as_ref() else {
        return ReplyV1::Unsupported;
    };
    if !staged_manifest_matches(&staged_root, application)
        || !staged_authorization_matches(&staged_root, action, &config)
        || !consume_helper_nonce(action, "launch")
    {
        return ReplyV1::Unsupported;
    }
    let executable = staged_root.join(&application.executable_relative_path);
    let Ok(bytes) = read_bounded(&executable, MAX_STAGE_BYTES) else {
        return ReplyV1::Unsupported;
    };
    if !valid_elf_executable(&bytes, application.guest_architecture)
        || !trusted_staged_executable(&executable, &staged_root)
        || !create_launch_marker(&session_root, &authorization.nonce)
    {
        return ReplyV1::Unsupported;
    }
    spawn_as_desktop(&executable)
}

pub(super) fn available() -> bool {
    let path = Path::new(STAGING_ROOT);
    let Ok(config) = super::catalog::helper_config() else {
        return false;
    };
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    use std::os::unix::fs::MetadataExt;
    let marker = Path::new("/var/lib/koolade-app-helper/boot-state.json");
    let Ok(marker_metadata) = fs::symlink_metadata(marker) else {
        return false;
    };
    metadata.file_type().is_dir()
        && metadata.uid() == 0
        && metadata.mode() & 0o022 == 0
        && marker_metadata.file_type().is_file()
        && marker_metadata.uid() == 0
        && !path.join(config.session_id).exists()
}

#[cfg(test)]
mod tests {
    use super::{GuestAppArchitectureV1 as Arch, valid_elf_executable};

    fn elf(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0; 20];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[16..18].copy_from_slice(&2_u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes
    }

    #[test]
    fn staged_executable_must_be_a_native_elf_for_the_guest_architecture() {
        assert!(valid_elf_executable(&elf(62), Arch::X86_64));
        assert!(valid_elf_executable(&elf(183), Arch::Aarch64));
        assert!(!valid_elf_executable(&elf(183), Arch::X86_64));
        assert!(!valid_elf_executable(
            b"#!/bin/sh\necho unsafe",
            Arch::X86_64
        ));
    }
}
