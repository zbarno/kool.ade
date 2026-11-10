//! Persistent local signing identity used to attest Computer Use sessions.
//! Only the public verification key is provisioned to remote workers.

use std::{fs, io::Read, path::Path};

use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

const KEY_FILE: &str = "computer-use/controller-key.json";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StoredComputerUseControllerKeyV1 {
    schema_version: u16,
    key_id: String,
    private_key_hex: String,
}

/// Load or create the operator-local signing key. The private seed is kept in
/// a 0600 file under a 0700 directory and is never returned as serialized
/// configuration or sent to a worker.
pub fn load_or_create_computer_use_controller_key_v1() -> anyhow::Result<(String, SigningKey)> {
    let path = crate::persistence::state_root().join(KEY_FILE);
    if path.exists() {
        return read_key(&path);
    }
    let parent = path.parent().expect("key file has parent");
    fs::create_dir_all(parent)?;
    set_private_dir(parent)?;
    let mut seed = [0_u8; 32];
    seed[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    seed[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    let key = SigningKey::from_bytes(&seed);
    seed.zeroize();
    let key_id = uuid::Uuid::new_v4().to_string();
    let mut private_key = key.to_bytes();
    let mut stored = StoredComputerUseControllerKeyV1 {
        schema_version: 1,
        key_id: key_id.clone(),
        private_key_hex: encode_hex(&private_key),
    };
    private_key.zeroize();
    let mut bytes = serde_json::to_vec(&stored)?;
    stored.private_key_hex.zeroize();
    let write = create_private_key_file(&path, &bytes);
    bytes.zeroize();
    match write {
        Ok(()) => {}
        Err(_) if path.exists() => return read_key(&path),
        Err(error) => return Err(error),
    }
    read_key(&path)
}

fn read_key(path: &Path) -> anyhow::Result<(String, SigningKey)> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "controller key is not a regular file"
    );
    anyhow::ensure!(metadata.len() <= 4096, "controller key file is oversized");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() },
            "controller key owner is invalid"
        );
        anyhow::ensure!(
            metadata.mode() & 0o077 == 0,
            "controller key permissions must be 0600 or stricter"
        );
    }
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 4096, "controller key file is oversized");
    let mut stored: StoredComputerUseControllerKeyV1 = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        stored.schema_version == 1 && canonical_uuid(&stored.key_id),
        "controller key identity is invalid"
    );
    let mut raw = decode_hex(&stored.private_key_hex)
        .ok_or_else(|| anyhow::anyhow!("controller key encoding is invalid"))?;
    stored.private_key_hex.zeroize();
    let mut seed: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("controller key length is invalid"))?;
    raw.zeroize();
    let key = SigningKey::from_bytes(&seed);
    seed.zeroize();
    Ok((stored.key_id, key))
}

fn set_private_dir(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(path)?;
        anyhow::ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == unsafe { libc::geteuid() },
            "controller key directory is unsafe"
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn create_private_key_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().expect("controller key parent");
    let temporary = parent.join(format!(".controller-key-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let result = (|| -> anyhow::Result<()> {
        use std::io::Write;
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::hard_link(&temporary, path)?;
        fs::remove_file(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 15) as usize] as char);
    }
    encoded
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| -> Option<u8> {
                Some(match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    b'A'..=b'F' => byte - b'A' + 10,
                    _ => return None,
                })
            };
            Some(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect()
}
