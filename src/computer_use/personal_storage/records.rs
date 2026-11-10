use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
};

use fs2::FileExt;

use super::{
    PersonalVmProfileStoreV1, PersonalVmProfileV1, PersonalVmStorageErrorV1,
    private::{private_create_new, valid_tagged_sha256, validate_private_file, validate_uuid},
};

const MAX_PROFILE_BYTES: u64 = 16 * 1024;

impl PersonalVmProfileStoreV1 {
    /// Lists only valid profiles owned by the authenticated local operator.
    /// A malformed profile aborts the listing rather than being silently skipped.
    pub fn list_owned_profiles(
        &self,
        owner_id: &str,
    ) -> Result<Vec<PersonalVmProfileV1>, PersonalVmStorageErrorV1> {
        validate_uuid(owner_id)?;
        let mut profiles = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|_| PersonalVmStorageErrorV1::Io)? {
            let entry = entry.map_err(|_| PersonalVmStorageErrorV1::Io)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(profile_id) = name.strip_suffix(".json") else {
                continue;
            };
            let profile = self.read_profile(profile_id)?;
            if profile.owner_id == owner_id {
                profiles.push(profile);
            }
        }
        profiles.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
        Ok(profiles)
    }

    pub(super) fn profile_path(&self, profile_id: &str) -> PathBuf {
        self.root.join(format!("{profile_id}.json"))
    }

    pub(super) fn write_profile(
        &self,
        profile: &PersonalVmProfileV1,
    ) -> Result<(), PersonalVmStorageErrorV1> {
        let bytes =
            serde_json::to_vec(profile).map_err(|_| PersonalVmStorageErrorV1::InvalidRecord)?;
        let path = self.profile_path(&profile.profile_id);
        let temp = self.root.join(format!(
            ".{}.{}.tmp",
            profile.profile_id,
            uuid::Uuid::new_v4()
        ));
        let mut file = private_create_new(&temp)?;
        file.write_all(&bytes)
            .map_err(|_| PersonalVmStorageErrorV1::Io)?;
        file.sync_all().map_err(|_| PersonalVmStorageErrorV1::Io)?;
        fs::rename(&temp, &path).map_err(|_| PersonalVmStorageErrorV1::Io)?;
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| PersonalVmStorageErrorV1::Io)
    }

    pub(super) fn read_profile(
        &self,
        profile_id: &str,
    ) -> Result<PersonalVmProfileV1, PersonalVmStorageErrorV1> {
        validate_uuid(profile_id)?;
        let path = self.profile_path(profile_id);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                PersonalVmStorageErrorV1::NotFound
            } else {
                PersonalVmStorageErrorV1::Io
            }
        })?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_PROFILE_BYTES {
            return Err(PersonalVmStorageErrorV1::InvalidRecord);
        }
        validate_private_file(&metadata)?;
        let bytes = fs::read(path).map_err(|_| PersonalVmStorageErrorV1::Io)?;
        let profile: PersonalVmProfileV1 =
            serde_json::from_slice(&bytes).map_err(|_| PersonalVmStorageErrorV1::InvalidRecord)?;
        if profile.schema_version != 1
            || profile.profile_id != profile_id
            || profile.disk_file_name != format!("{profile_id}.qcow2")
            || profile.retention_until_unix_ms <= profile.created_at_unix_ms
            || profile.catalog_generation == 0
            || !(2 * 1024 * 1024 * 1024..=16 * 1024 * 1024 * 1024)
                .contains(&profile.virtual_size_bytes)
            || !valid_tagged_sha256(&profile.image_digest)
            || !valid_tagged_sha256(&profile.image_manifest_digest)
        {
            return Err(PersonalVmStorageErrorV1::InvalidRecord);
        }
        validate_uuid(&profile.owner_id)?;
        validate_uuid(&profile.image_signer_key_id)?;
        Ok(profile)
    }

    pub(super) fn acquire_profile_lock(
        &self,
        profile_id: &str,
    ) -> Result<File, PersonalVmStorageErrorV1> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        validate_uuid(profile_id)?;
        let path = self.root.join(format!("{profile_id}.lock"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|_| PersonalVmStorageErrorV1::LockUnavailable)?;
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| PersonalVmStorageErrorV1::LockUnavailable)?;
        if !metadata.file_type().is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(PersonalVmStorageErrorV1::UnsafeDirectory);
        }
        file.try_lock_exclusive()
            .map_err(|_| PersonalVmStorageErrorV1::ProfileLocked)?;
        Ok(file)
    }
}
