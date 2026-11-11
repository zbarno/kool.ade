use std::fs;

use super::{
    PersonalVmCreateRequestV1, PersonalVmDiskKeyV1, PersonalVmProfileStoreV1, PersonalVmProfileV1,
    PersonalVmStorageErrorV1,
    private::{random_key, validate_private_file, validate_uuid},
};

impl PersonalVmProfileStoreV1 {
    /// Applies retention on each owner inventory refresh. Expired disks are
    /// crypto-erased immediately unless an active session still holds the lock.
    pub fn expire_owned_profiles(
        &self,
        owner_id: &str,
        now_unix_ms: u64,
    ) -> Result<(), PersonalVmStorageErrorV1> {
        for profile in self.list_owned_profiles(owner_id)? {
            if profile.retention_until_unix_ms <= now_unix_ms {
                match self.delete_profile(owner_id, &profile.profile_id) {
                    Ok(()) | Err(PersonalVmStorageErrorV1::ProfileLocked) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }

    /// Removes only expired profiles belonging to this operator. An active VM
    /// holds the exclusive profile lock, so expiration cannot erase its key.
    pub fn delete_expired_profiles(
        &self,
        owner_id: &str,
        now_unix_ms: u64,
    ) -> Result<Vec<String>, PersonalVmStorageErrorV1> {
        let expired = self
            .list_owned_profiles(owner_id)?
            .into_iter()
            .filter(|profile| profile.retention_until_unix_ms <= now_unix_ms)
            .map(|profile| profile.profile_id)
            .collect::<Vec<_>>();
        for profile_id in &expired {
            self.delete_profile(owner_id, profile_id)?;
        }
        Ok(expired)
    }

    /// Creates only the owner-bound metadata/key. Disk creation is a separate
    /// transaction so callers can roll the key back if encrypted qcow2 setup fails.
    fn create_profile(
        &self,
        owner_id: &str,
        profile_id: &str,
        now_unix_ms: u64,
        retention_until_unix_ms: u64,
        image: &crate::computer_use::guest_image::CachedGuestImageV1,
    ) -> Result<(PersonalVmProfileV1, PersonalVmDiskKeyV1), PersonalVmStorageErrorV1> {
        validate_uuid(owner_id)?;
        if retention_until_unix_ms <= now_unix_ms {
            return Err(PersonalVmStorageErrorV1::InvalidRecord);
        }
        validate_uuid(profile_id)?;
        if self.profile_path(profile_id).exists() {
            return Err(PersonalVmStorageErrorV1::AlreadyExists);
        }
        let profile = PersonalVmProfileV1 {
            schema_version: 1,
            profile_id: profile_id.to_owned(),
            owner_id: owner_id.to_owned(),
            created_at_unix_ms: now_unix_ms,
            retention_until_unix_ms,
            disk_file_name: format!("{profile_id}.qcow2"),
            image_digest: image.entry().image_digest.clone(),
            image_manifest_digest: image.manifest_digest().to_owned(),
            image_signer_key_id: image.manifest_signer_key_id().to_owned(),
            catalog_generation: image.catalog_generation(),
            virtual_size_bytes: image.entry().image_size_bytes,
        };
        let key = random_key()?;
        self.key_service
            .store_key(owner_id, profile_id, key.expose_for_qemu_secret_fd())?;
        if let Err(error) = self.write_profile(&profile) {
            let _ = self.key_service.delete_key(owner_id, profile_id);
            return Err(error);
        }
        Ok((profile, key))
    }

    fn create_encrypted_profile(
        &self,
        owner_id: &str,
        profile_id: &str,
        now_unix_ms: u64,
        retention_until_unix_ms: u64,
        image: &crate::computer_use::guest_image::CachedGuestImageV1,
        qemu_tool: &crate::computer_use::guest_image::QemuImageToolV1,
    ) -> Result<(PersonalVmProfileV1, PersonalVmDiskKeyV1), PersonalVmStorageErrorV1> {
        let (profile, key) = self.create_profile(
            owner_id,
            profile_id,
            now_unix_ms,
            retention_until_unix_ms,
            image,
        )?;
        let disk_path = self.root.join(&profile.disk_file_name);
        if crate::computer_use::guest_image::create_encrypted_personal_disk(
            qemu_tool,
            image.path(),
            &disk_path,
            &key,
            image.entry().image_size_bytes,
        )
        .is_err()
        {
            if self
                .revoke_profile_key(owner_id, &profile.profile_id)
                .is_ok()
            {
                let _ = fs::remove_file(&disk_path);
                let _ = self.remove_profile_record(owner_id, &profile.profile_id);
            }
            return Err(PersonalVmStorageErrorV1::DiskCreationFailed);
        }
        Ok((profile, key))
    }

    pub(crate) fn create_encrypted_session_disk(
        &self,
        owner_id: &str,
        request: PersonalVmCreateRequestV1<'_>,
    ) -> Result<
        (
            PersonalVmProfileV1,
            crate::computer_use::guest_image::GuestImageOverlayV1,
        ),
        PersonalVmStorageErrorV1,
    > {
        let PersonalVmCreateRequestV1 {
            profile_id,
            session_id,
            now_unix_ms,
            retention_until_unix_ms,
            image,
            qemu_tool,
        } = request;
        validate_uuid(session_id)?;
        let (profile, key) = self.create_encrypted_profile(
            owner_id,
            profile_id,
            now_unix_ms,
            retention_until_unix_ms,
            image,
            qemu_tool,
        )?;
        let lock = match self.acquire_profile_lock(&profile.profile_id) {
            Ok(lock) => lock,
            Err(error) => {
                let _ = self.delete_profile(owner_id, &profile.profile_id);
                return Err(error);
            }
        };
        let overlay =
            match crate::computer_use::guest_image::GuestImageOverlayV1::from_personal_profile(
                session_id.to_owned(),
                &profile,
                image,
                &self.root,
                key,
                lock,
            ) {
                Ok(overlay) => overlay,
                Err(_) => {
                    let _ = self.delete_profile(owner_id, &profile.profile_id);
                    return Err(PersonalVmStorageErrorV1::DiskCreationFailed);
                }
            };
        Ok((profile, overlay))
    }

    /// Crypto-erases first. Callers remove the disk only after this succeeds.
    fn revoke_profile_key(
        &self,
        owner_id: &str,
        profile_id: &str,
    ) -> Result<(), PersonalVmStorageErrorV1> {
        validate_uuid(owner_id)?;
        validate_uuid(profile_id)?;
        let profile = self.read_profile(profile_id)?;
        if profile.owner_id != owner_id {
            return Err(PersonalVmStorageErrorV1::WrongOwner);
        }
        self.key_service.delete_key(owner_id, profile_id)
    }

    fn remove_profile_record(
        &self,
        owner_id: &str,
        profile_id: &str,
    ) -> Result<(), PersonalVmStorageErrorV1> {
        validate_uuid(owner_id)?;
        validate_uuid(profile_id)?;
        let profile = self.read_profile(profile_id)?;
        if profile.owner_id != owner_id {
            return Err(PersonalVmStorageErrorV1::WrongOwner);
        }
        fs::remove_file(self.profile_path(profile_id)).map_err(|_| PersonalVmStorageErrorV1::Io)
    }

    /// Key erasure happens before disk removal. If removal fails, the encrypted
    /// file remains unreadable and the owner-bound record stays for cleanup retry.
    pub fn delete_profile(
        &self,
        owner_id: &str,
        profile_id: &str,
    ) -> Result<(), PersonalVmStorageErrorV1> {
        let _lock = self.acquire_profile_lock(profile_id)?;
        let profile = self.read_owned_profile(owner_id, profile_id)?;
        self.key_service.delete_key(owner_id, profile_id)?;
        let disk_path = self.root.join(&profile.disk_file_name);
        match fs::symlink_metadata(&disk_path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                validate_private_file(&metadata)?;
                fs::remove_file(&disk_path).map_err(|_| PersonalVmStorageErrorV1::Io)?;
            }
            Ok(_) => return Err(PersonalVmStorageErrorV1::UnsafeDirectory),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(PersonalVmStorageErrorV1::Io),
        }
        self.remove_profile_record(owner_id, profile_id)
    }

    fn load_owned_profile(
        &self,
        owner_id: &str,
        profile_id: &str,
        now_unix_ms: u64,
    ) -> Result<(PersonalVmProfileV1, PersonalVmDiskKeyV1), PersonalVmStorageErrorV1> {
        let profile = self.read_owned_profile(owner_id, profile_id)?;
        if profile.retention_until_unix_ms <= now_unix_ms {
            return Err(PersonalVmStorageErrorV1::NotFound);
        }
        let disk_path = self.root.join(&profile.disk_file_name);
        let metadata =
            fs::symlink_metadata(&disk_path).map_err(|_| PersonalVmStorageErrorV1::NotFound)?;
        if !metadata.file_type().is_file() {
            return Err(PersonalVmStorageErrorV1::UnsafeDirectory);
        }
        validate_private_file(&metadata)?;
        let key = self.key_service.load_key(owner_id, profile_id)?;
        Ok((profile, key))
    }

    /// Unlocks an existing profile for exactly one caller-owned session. The
    /// returned overlay owns the volatile key; dropping it zeroizes that copy.
    pub fn unlock_profile_for_session(
        &self,
        owner_id: &str,
        profile_id: &str,
        session_id: &str,
        now_unix_ms: u64,
        image: &crate::computer_use::guest_image::CachedGuestImageV1,
    ) -> Result<crate::computer_use::guest_image::GuestImageOverlayV1, PersonalVmStorageErrorV1>
    {
        validate_uuid(session_id)?;
        let lock = self.acquire_profile_lock(profile_id)?;
        let (profile, key) = self.load_owned_profile(owner_id, profile_id, now_unix_ms)?;
        if profile.image_digest != image.entry().image_digest
            || profile.image_manifest_digest != image.manifest_digest()
            || profile.image_signer_key_id != image.manifest_signer_key_id()
            || profile.catalog_generation != image.catalog_generation()
        {
            return Err(PersonalVmStorageErrorV1::InvalidRecord);
        }
        crate::computer_use::guest_image::GuestImageOverlayV1::from_personal_profile(
            session_id.to_owned(),
            &profile,
            image,
            &self.root,
            key,
            lock,
        )
        .map_err(|_| PersonalVmStorageErrorV1::InvalidRecord)
    }

    pub(super) fn read_owned_profile(
        &self,
        owner_id: &str,
        profile_id: &str,
    ) -> Result<PersonalVmProfileV1, PersonalVmStorageErrorV1> {
        validate_uuid(owner_id)?;
        validate_uuid(profile_id)?;
        let profile = self.read_profile(profile_id)?;
        if profile.owner_id != owner_id {
            return Err(PersonalVmStorageErrorV1::WrongOwner);
        }
        Ok(profile)
    }
}
