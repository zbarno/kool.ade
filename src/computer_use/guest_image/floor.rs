use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt;
use uuid::Uuid;

use super::cache::ImageCacheErrorV1;

const STATE_FILE: &str = "catalog-generation-v1";
const LOCK_FILE: &str = "catalog-generation.lock";
const STATE_HEADER: &str = "koolade-computer-use-catalog-generation-v1\n";

/// Persistent app-owned floor preventing replay of older signed catalogs.
/// The root must live in durable application state, outside disposable caches.
#[derive(Debug)]
pub struct GuestImageCatalogGenerationFloorV1 {
    root: PathBuf,
}

impl GuestImageCatalogGenerationFloorV1 {
    /// Use only during authenticated first-install provisioning with the
    /// minimum generation shipped in the signed app release. Existing state is
    /// never reset or replaced by this method.
    pub fn initialize_for_first_install(
        root: PathBuf,
        minimum_generation: u64,
    ) -> Result<Self, ImageCacheErrorV1> {
        if minimum_generation == 0 {
            return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
        }
        prepare_root(&root)?;
        let floor = Self { root };
        let lock = floor.lock(true)?;
        match fs::symlink_metadata(floor.state_path()) {
            Ok(_) => return Err(ImageCacheErrorV1::CatalogFloorUnavailable),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ImageCacheErrorV1::CatalogFloorUnavailable),
        }
        floor.write_generation(minimum_generation)?;
        FileExt::unlock(&lock).map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
        Ok(floor)
    }

    /// Open existing durable state. Missing, malformed, or unsafe state fails
    /// closed; callers must not recreate it from a cache or task-provided value.
    pub fn open(root: PathBuf) -> Result<Self, ImageCacheErrorV1> {
        validate_root(&root)?;
        let floor = Self { root };
        let lock = floor.lock(false)?;
        floor.read_generation()?;
        FileExt::unlock(&lock).map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
        Ok(floor)
    }

    pub(super) fn accept_generation(&self, generation: u64) -> Result<(), ImageCacheErrorV1> {
        if generation == 0 {
            return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
        }
        let lock = self.lock(false)?;
        let current = self.read_generation()?;
        if generation < current {
            return Err(ImageCacheErrorV1::CatalogRollback);
        }
        if generation > current {
            self.write_generation(generation)?;
        }
        FileExt::unlock(&lock).map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)
    }

    fn lock(&self, create: bool) -> Result<File, ImageCacheErrorV1> {
        validate_root(&self.root)?;
        let path = self.root.join(LOCK_FILE);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(create);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(&path)
            .map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
        let metadata = file
            .metadata()
            .map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
        if !metadata.is_file() {
            return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != unsafe { libc::geteuid() }
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
            }
        }
        FileExt::lock_exclusive(&file).map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
        Ok(file)
    }

    fn read_generation(&self) -> Result<u64, ImageCacheErrorV1> {
        let path = self.state_path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
        if !metadata.file_type().is_file() {
            return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != unsafe { libc::geteuid() }
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
            }
        }
        let mut bytes = Vec::new();
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        options
            .open(path)
            .and_then(|file| file.take(128).read_to_end(&mut bytes))
            .map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
        let value =
            std::str::from_utf8(&bytes).map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
        let Some(number) = value
            .strip_prefix(STATE_HEADER)
            .and_then(|v| v.strip_suffix('\n'))
        else {
            return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
        };
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
        }
        number
            .parse::<u64>()
            .ok()
            .filter(|generation| *generation > 0)
            .ok_or(ImageCacheErrorV1::CatalogFloorUnavailable)
    }

    fn write_generation(&self, generation: u64) -> Result<(), ImageCacheErrorV1> {
        let temporary = self
            .root
            .join(format!(".catalog-floor-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
            writeln!(file, "{STATE_HEADER}{generation}")
                .and_then(|_| file.sync_all())
                .map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
            fs::rename(&temporary, self.state_path())
                .map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
            File::open(&self.root)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(STATE_FILE)
    }
}

fn prepare_root(root: &PathBuf) -> Result<(), ImageCacheErrorV1> {
    fs::create_dir(root).map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))
            .map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
    }
    validate_root(root)
}

fn validate_root(root: &Path) -> Result<(), ImageCacheErrorV1> {
    let metadata =
        fs::symlink_metadata(root).map_err(|_| ImageCacheErrorV1::CatalogFloorUnavailable)?;
    if !metadata.file_type().is_dir() {
        return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
        }
    }
    #[cfg(not(unix))]
    {
        return Err(ImageCacheErrorV1::CatalogFloorUnavailable);
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "floor/tests.rs"]
mod tests;
