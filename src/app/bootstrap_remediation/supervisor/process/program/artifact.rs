use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use crate::app::bootstrap_remediation::plan::{ArtifactTrust, RemediationPlan};

pub(crate) struct VerifiedArtifact {
    pub(super) path: PathBuf,
    pub(super) file: File,
    pub(super) sha256: String,
}

pub(crate) trait PublisherSignatureVerifier {
    fn verify(
        &self,
        file: &File,
        source: &crate::app::bootstrap_remediation::plan::ArtifactSource,
        trust: &ArtifactTrust,
    ) -> anyhow::Result<()>;
}

pub(crate) fn verify_plan_artifact(
    plan: &RemediationPlan,
    path: &Path,
    signature_verifier: &dyn PublisherSignatureVerifier,
) -> anyhow::Result<VerifiedArtifact> {
    plan.validate()?;
    let trust = plan
        .artifact_trust
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("remediation plan has no verified artifact"))?;
    anyhow::ensure!(
        plan.artifact_source.is_some(),
        "artifact has no approved source"
    );
    let opened = open_regular_file(path)?;
    let actual = hash_open_file(&opened)?;
    let expected = match trust {
        ArtifactTrust::DigestPinned { sha256 } | ArtifactTrust::PublisherSigned { sha256, .. } => {
            sha256
        }
    };
    anyhow::ensure!(
        actual == expected.to_ascii_lowercase(),
        "downloaded artifact does not match its reviewed SHA-256 digest"
    );
    if matches!(trust, ArtifactTrust::PublisherSigned { .. }) {
        signature_verifier.verify(&opened, plan.artifact_source.as_ref().unwrap(), trust)?;
    }
    let file = sealed_verified_copy(opened, &actual)?;
    Ok(VerifiedArtifact {
        path: path.to_owned(),
        file,
        sha256: actual,
    })
}

impl VerifiedArtifact {
    pub(crate) fn digest(&self) -> &str {
        &self.sha256
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            hash_open_file(&self.file)? == self.sha256,
            "verified installer artifact changed before execution"
        );
        Ok(())
    }

    pub(super) fn execution_path(&self) -> OsString {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            format!("/proc/self/fd/{}", self.file.as_raw_fd()).into()
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::AsRawFd;
            format!("/dev/fd/{}", self.file.as_raw_fd()).into()
        }
        #[cfg(windows)]
        {
            self.path.as_os_str().to_owned()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            self.path.as_os_str().to_owned()
        }
    }
}

fn open_regular_file(path: &Path) -> anyhow::Result<File> {
    anyhow::ensure!(
        path.is_absolute(),
        "installer artifact path is not absolute"
    );
    let link_metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        link_metadata.is_file() && !link_metadata.file_type().is_symlink(),
        "installer artifact is not a regular file"
    );
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?
    };
    #[cfg(not(unix))]
    let file = {
        #[cfg(windows)]
        use std::os::windows::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        options
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
        options.open(path)?
    };
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= 2 * 1024 * 1024 * 1024,
        "installer artifact size is outside policy"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            metadata.nlink() == 1 && metadata.mode() & 0o222 == 0,
            "installer artifact is linked or writable"
        );
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("installer artifact has no private parent"))?;
        let parent_metadata = std::fs::symlink_metadata(parent)?;
        anyhow::ensure!(
            parent_metadata.is_dir()
                && !parent_metadata.file_type().is_symlink()
                && parent_metadata.uid() == unsafe { libc::geteuid() }
                && parent_metadata.mode() & 0o077 == 0,
            "installer artifact parent is not a private application directory"
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        anyhow::ensure!(
            metadata.file_attributes()
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                == 0,
            "installer artifact is a reparse point"
        );
    }
    anyhow::ensure!(
        path.canonicalize()? == path,
        "installer artifact path changed while it was verified"
    );
    Ok(file)
}

fn hash_open_file(file: &File) -> anyhow::Result<String> {
    let mut file = file.try_clone()?;
    file.seek(SeekFrom::Start(0))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(target_os = "linux")]
fn sealed_verified_copy(source: File, expected_digest: &str) -> anyhow::Result<File> {
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd};

    const MFD_CLOEXEC: u32 = 0x0001;
    const MFD_ALLOW_SEALING: u32 = 0x0002;
    const F_ADD_SEALS: i32 = 1033;
    const F_SEAL_SEAL: i32 = 0x0001;
    const F_SEAL_SHRINK: i32 = 0x0002;
    const F_SEAL_GROW: i32 = 0x0004;
    const F_SEAL_WRITE: i32 = 0x0008;
    let name = std::ffi::CString::new("koolade-verified-installer")?;
    let descriptor = unsafe { libc::memfd_create(name.as_ptr(), MFD_CLOEXEC | MFD_ALLOW_SEALING) };
    anyhow::ensure!(
        descriptor >= 0,
        "could not create sealed installer artifact"
    );
    let mut writable = unsafe { File::from_raw_fd(descriptor) };
    let mut source = source;
    source.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut source, &mut writable)?;
    writable.flush()?;
    anyhow::ensure!(
        hash_open_file(&writable)? == expected_digest,
        "installer payload changed while it was copied"
    );
    anyhow::ensure!(
        unsafe {
            libc::fcntl(
                descriptor,
                F_ADD_SEALS,
                F_SEAL_SEAL | F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE,
            )
        } == 0,
        "could not seal the verified installer artifact"
    );
    let fd = writable.as_raw_fd();
    let path = format!("/proc/self/fd/{fd}");
    let readonly = OpenOptions::new().read(true).open(path)?;
    drop(writable);
    drop(source);
    Ok(readonly)
}

#[cfg(windows)]
fn sealed_verified_copy(source: File, _expected_digest: &str) -> anyhow::Result<File> {
    Ok(source)
}

#[cfg(target_os = "macos")]
fn sealed_verified_copy(_source: File, _expected_digest: &str) -> anyhow::Result<File> {
    anyhow::bail!(
        "macOS installer handoff is not enabled until its immutable artifact adapter exists"
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn sealed_verified_copy(_source: File, _expected_digest: &str) -> anyhow::Result<File> {
    anyhow::bail!("immutable installer artifact handoff is unavailable on this platform")
}
