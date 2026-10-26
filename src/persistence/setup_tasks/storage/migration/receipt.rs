use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use super::{copy::TreeDigest, permissions};

const RECEIPT: &str = ".koolade-machine-setup-migration-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReceiptSource {
    None,
    LegacyCopy,
}

pub(super) struct MigrationReceipt {
    source: ReceiptSource,
    sha256: Option<String>,
    files: usize,
    bytes: u64,
}

pub(super) fn read(directory: &Path) -> anyhow::Result<Option<MigrationReceipt>> {
    let path = directory.join(RECEIPT);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !permissions::is_link_or_reparse(&metadata),
                "unsafe machine setup migration receipt"
            );
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
                options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
            }
            let mut file = options.open(&path)?;
            let opened_metadata = file.metadata()?;
            anyhow::ensure!(
                opened_metadata.is_file() && !permissions::is_link_or_reparse(&opened_metadata),
                "unsafe machine setup migration receipt"
            );
            permissions::secure_private_file(&path)?;
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            let mut lines = content.lines();
            anyhow::ensure!(
                lines.next() == Some("koolade-machine-setup-v1"),
                "invalid machine setup migration receipt"
            );
            let (mut source, mut sha256, mut files, mut bytes) = (None, None, None, None);
            for line in lines {
                if let Some(value) = line.strip_prefix("source=") {
                    anyhow::ensure!(source.replace(value.to_owned()).is_none());
                } else if let Some(value) = line.strip_prefix("sha256=") {
                    anyhow::ensure!(sha256.replace(value.to_owned()).is_none());
                } else if let Some(value) = line.strip_prefix("files=") {
                    anyhow::ensure!(files.replace(value.parse::<usize>()?).is_none());
                } else if let Some(value) = line.strip_prefix("bytes=") {
                    anyhow::ensure!(bytes.replace(value.parse::<u64>()?).is_none());
                } else {
                    anyhow::bail!("invalid machine setup migration receipt field");
                }
            }
            let files =
                files.ok_or_else(|| anyhow::anyhow!("migration receipt has no file count"))?;
            let bytes =
                bytes.ok_or_else(|| anyhow::anyhow!("migration receipt has no byte count"))?;
            let receipt = match (source.as_deref(), sha256.as_deref()) {
                (Some("none"), Some("none")) => MigrationReceipt {
                    source: ReceiptSource::None,
                    sha256: None,
                    files,
                    bytes,
                },
                (Some("legacy-copy"), Some(value))
                    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
                {
                    MigrationReceipt {
                        source: ReceiptSource::LegacyCopy,
                        sha256: Some(value.to_owned()),
                        files,
                        bytes,
                    }
                }
                _ => anyhow::bail!("invalid machine setup migration receipt source or digest"),
            };
            Ok(Some(receipt))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn validate_legacy_snapshot(
    receipt: &MigrationReceipt,
    legacy_digest: &TreeDigest,
) -> anyhow::Result<()> {
    match receipt.source {
        ReceiptSource::None => anyhow::ensure!(
            receipt.files == 0
                && receipt.bytes == 0
                && legacy_digest.files == 0
                && legacy_digest.bytes == 0,
            "legacy machine setup state appeared after platform state was initialized; resolve the state conflict before continuing"
        ),
        ReceiptSource::LegacyCopy => anyhow::ensure!(
            receipt.sha256.as_deref() == Some(legacy_digest.hash.as_str())
                && receipt.files == legacy_digest.files
                && receipt.bytes == legacy_digest.bytes,
            "legacy machine setup state changed after migration; resolve the state conflict before continuing"
        ),
    }
    Ok(())
}

pub(super) fn write_fresh(directory: &Path) -> anyhow::Result<()> {
    write(directory, "source=none\nsha256=none\nfiles=0\nbytes=0\n")
}

pub(super) fn write(directory: &Path, details: &str) -> anyhow::Result<()> {
    let path = directory.join(RECEIPT);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path)?;
    file.write_all(format!("koolade-machine-setup-v1\n{details}").as_bytes())?;
    file.sync_all()?;
    permissions::secure_private_file(&path)
}
