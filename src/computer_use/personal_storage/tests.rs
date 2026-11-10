use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};

use super::{
    PersonalVmDiskKeyV1, PersonalVmProfileStoreV1, PersonalVmProfileV1, PersonalVmStorageErrorV1,
    key_service::PersonalVmKeyServiceV1,
};
use zeroize::Zeroizing;

const OWNER: &str = "00000000-0000-4000-8000-000000000001";
const OTHER_OWNER: &str = "00000000-0000-4000-8000-000000000002";
const PROFILE: &str = "00000000-0000-4000-8000-000000000003";

#[derive(Default)]
struct FakeKeyService {
    keys: Mutex<BTreeMap<(String, String), [u8; 32]>>,
    disk_path: Mutex<Option<std::path::PathBuf>>,
}

impl PersonalVmKeyServiceV1 for FakeKeyService {
    fn store_key(
        &self,
        owner: &str,
        profile: &str,
        key: &[u8; 32],
    ) -> Result<(), PersonalVmStorageErrorV1> {
        self.keys
            .lock()
            .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?
            .insert((owner.to_owned(), profile.to_owned()), *key);
        Ok(())
    }

    fn load_key(
        &self,
        owner: &str,
        profile: &str,
    ) -> Result<PersonalVmDiskKeyV1, PersonalVmStorageErrorV1> {
        self.keys
            .lock()
            .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?
            .get(&(owner.to_owned(), profile.to_owned()))
            .copied()
            .map(|key| PersonalVmDiskKeyV1(Zeroizing::new(key)))
            .ok_or(PersonalVmStorageErrorV1::KeyServiceUnavailable)
    }

    fn delete_key(&self, owner: &str, profile: &str) -> Result<(), PersonalVmStorageErrorV1> {
        if let Some(path) = self.disk_path.lock().unwrap().as_ref() {
            assert!(path.exists(), "key must be revoked before disk removal");
        }
        self.keys
            .lock()
            .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?
            .remove(&(owner.to_owned(), profile.to_owned()))
            .map(|_| ())
            .ok_or(PersonalVmStorageErrorV1::KeyServiceUnavailable)
    }
}

fn profile() -> PersonalVmProfileV1 {
    PersonalVmProfileV1 {
        schema_version: 1,
        profile_id: PROFILE.into(),
        owner_id: OWNER.into(),
        created_at_unix_ms: 1,
        retention_until_unix_ms: 10_000,
        disk_file_name: format!("{PROFILE}.qcow2"),
        image_digest: format!("sha256:{}", "a".repeat(64)),
        image_manifest_digest: format!("sha256:{}", "b".repeat(64)),
        image_signer_key_id: OWNER.into(),
        catalog_generation: 1,
        virtual_size_bytes: 2 * 1024 * 1024 * 1024,
    }
}

fn store(root: &std::path::Path, keys: Arc<FakeKeyService>) -> PersonalVmProfileStoreV1 {
    fs::create_dir_all(root).unwrap();
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    PersonalVmProfileStoreV1 {
        root: root.to_path_buf(),
        key_service: keys,
    }
}

#[test]
fn profile_owner_segregation_precedes_key_lookup() {
    let temp = tempfile::tempdir().unwrap();
    let keys = Arc::new(FakeKeyService::default());
    let store = store(temp.path(), keys.clone());
    store.write_profile(&profile()).unwrap();

    assert_eq!(
        store.read_owned_profile(OTHER_OWNER, PROFILE),
        Err(PersonalVmStorageErrorV1::WrongOwner)
    );
    assert!(keys.keys.lock().unwrap().is_empty());
}

#[test]
fn owner_listing_and_retention_cleanup_are_scoped_and_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let keys = Arc::new(FakeKeyService::default());
    let store = store(temp.path(), keys.clone());
    let mut expired = profile();
    expired.retention_until_unix_ms = 20;
    store.write_profile(&expired).unwrap();
    let disk = temp.path().join(format!("{PROFILE}.qcow2"));
    fs::write(&disk, b"encrypted qcow2").unwrap();
    fs::set_permissions(&disk, fs::Permissions::from_mode(0o600)).unwrap();
    keys.store_key(OWNER, PROFILE, &[3; 32]).unwrap();

    assert_eq!(store.list_owned_profiles(OWNER).unwrap(), vec![expired]);
    assert!(store.list_owned_profiles(OTHER_OWNER).unwrap().is_empty());
    assert_eq!(
        store.delete_expired_profiles(OTHER_OWNER, 20).unwrap(),
        Vec::<String>::new()
    );
    let active_lock = store.acquire_profile_lock(PROFILE).unwrap();
    assert_eq!(
        store.delete_expired_profiles(OWNER, 20),
        Err(PersonalVmStorageErrorV1::ProfileLocked),
        "retention cleanup cannot revoke a key while its VM is unlocked"
    );
    store.expire_owned_profiles(OWNER, 20).unwrap();
    assert!(disk.exists());
    assert!(
        keys.keys
            .lock()
            .unwrap()
            .contains_key(&(OWNER.into(), PROFILE.into()))
    );
    drop(active_lock);
    assert_eq!(
        store.delete_expired_profiles(OWNER, 20).unwrap(),
        vec![PROFILE.to_owned()]
    );
    assert!(!disk.exists());
    assert!(store.list_owned_profiles(OWNER).unwrap().is_empty());
}

#[test]
fn delete_profile_revokes_key_before_removing_encrypted_disk_and_record() {
    let temp = tempfile::tempdir().unwrap();
    let keys = Arc::new(FakeKeyService::default());
    keys.store_key(OWNER, PROFILE, &[7; 32]).unwrap();
    let store = store(temp.path(), keys.clone());
    store.write_profile(&profile()).unwrap();
    let disk = temp.path().join(format!("{PROFILE}.qcow2"));
    fs::write(&disk, b"ciphertext").unwrap();
    fs::set_permissions(&disk, fs::Permissions::from_mode(0o600)).unwrap();
    *keys.disk_path.lock().unwrap() = Some(disk.clone());

    store.delete_profile(OWNER, PROFILE).unwrap();

    assert!(!disk.exists());
    assert!(!store.profile_path(PROFILE).exists());
    assert!(keys.keys.lock().unwrap().is_empty());
}

#[test]
fn profile_lock_is_exclusive_and_released_with_its_guard() {
    let temp = tempfile::tempdir().unwrap();
    let service = Arc::new(FakeKeyService::default());
    let first = store(temp.path(), service.clone());
    let second = store(temp.path(), service);

    let lock = first.acquire_profile_lock(PROFILE).unwrap();
    assert_eq!(
        second.acquire_profile_lock(PROFILE).unwrap_err(),
        PersonalVmStorageErrorV1::ProfileLocked
    );
    drop(lock);
    assert!(second.acquire_profile_lock(PROFILE).is_ok());
}

#[test]
fn personal_overlay_release_preserves_disk_and_unlocks_only_after_key_owner_drops() {
    let temp = tempfile::tempdir().unwrap();
    let service = Arc::new(FakeKeyService::default());
    let store = store(temp.path(), service);
    store.write_profile(&profile()).unwrap();
    let disk = temp.path().join(format!("{PROFILE}.qcow2"));
    fs::write(&disk, b"encrypted qcow2").unwrap();
    fs::set_permissions(&disk, fs::Permissions::from_mode(0o600)).unwrap();
    let lock = store.acquire_profile_lock(PROFILE).unwrap();
    let overlay =
        crate::computer_use::guest_image::GuestImageOverlayV1::for_persistent_cleanup_test(
            disk.clone(),
            temp.path().to_path_buf(),
            PersonalVmDiskKeyV1(Zeroizing::new([9; 32])),
            lock,
        );

    overlay.release_after_vm_stopped().unwrap();
    assert!(
        disk.is_file(),
        "locking a VM retains its encrypted profile disk"
    );
    assert_eq!(
        store.acquire_profile_lock(PROFILE).unwrap_err(),
        PersonalVmStorageErrorV1::ProfileLocked,
        "the live overlay retains the profile lock until its volatile key is dropped"
    );
    drop(overlay);
    assert!(store.acquire_profile_lock(PROFILE).is_ok());
    assert!(disk.is_file());
}

#[test]
fn qemu_secret_fd_contains_only_the_ephemeral_raw_key() {
    use std::io::Read;
    let key = PersonalVmDiskKeyV1(Zeroizing::new([0x5a; 32]));
    let mut fd = key.as_qemu_secret_fd().unwrap();
    let mut bytes = Vec::new();
    fd.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, [0x5a; 32]);
}
