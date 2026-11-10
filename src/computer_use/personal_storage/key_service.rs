use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use zeroize::Zeroizing;

use super::{KEY_SERVICE, PersonalVmDiskKeyV1, PersonalVmStorageErrorV1, private::validate_uuid};

pub(super) trait PersonalVmKeyServiceV1: Send + Sync {
    fn store_key(
        &self,
        owner: &str,
        profile: &str,
        key: &[u8; 32],
    ) -> Result<(), PersonalVmStorageErrorV1>;
    fn load_key(
        &self,
        owner: &str,
        profile: &str,
    ) -> Result<PersonalVmDiskKeyV1, PersonalVmStorageErrorV1>;
    fn delete_key(&self, owner: &str, profile: &str) -> Result<(), PersonalVmStorageErrorV1>;
}

pub(super) struct SecretToolKeyServiceV1 {
    executable: PathBuf,
}

impl SecretToolKeyServiceV1 {
    pub(super) fn new(executable: PathBuf) -> Self {
        Self { executable }
    }

    fn command(
        &self,
        verb: &str,
        owner: &str,
        profile: &str,
    ) -> Result<Command, PersonalVmStorageErrorV1> {
        validate_uuid(owner)?;
        validate_uuid(profile)?;
        let mut command = Command::new(&self.executable);
        command.arg(verb);
        if verb == "store" {
            command.arg("--label=Koolade personal VM key");
        }
        command
            .arg("service")
            .arg(KEY_SERVICE)
            .arg("owner")
            .arg(owner)
            .arg("profile")
            .arg(profile);
        Ok(command)
    }
}

impl PersonalVmKeyServiceV1 for SecretToolKeyServiceV1 {
    fn store_key(
        &self,
        owner: &str,
        profile: &str,
        key: &[u8; 32],
    ) -> Result<(), PersonalVmStorageErrorV1> {
        let mut child = self.command("store", owner, profile)?;
        let encoded = Zeroizing::new(STANDARD.encode(key));
        child
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = child
            .spawn()
            .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
        process
            .stdin
            .take()
            .ok_or(PersonalVmStorageErrorV1::KeyServiceUnavailable)?
            .write_all(encoded.as_bytes())
            .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
        process
            .wait()
            .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?
            .success()
            .then_some(())
            .ok_or(PersonalVmStorageErrorV1::KeyServiceUnavailable)
    }

    fn load_key(
        &self,
        owner: &str,
        profile: &str,
    ) -> Result<PersonalVmDiskKeyV1, PersonalVmStorageErrorV1> {
        let output = self
            .command("lookup", owner, profile)?
            .output()
            .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
        if !output.status.success() || output.stdout.len() > 128 {
            return Err(PersonalVmStorageErrorV1::KeyServiceUnavailable);
        }
        let stdout = Zeroizing::new(output.stdout);
        let encoded = std::str::from_utf8(&stdout)
            .map_err(|_| PersonalVmStorageErrorV1::InvalidKey)?
            .trim();
        let decoded = Zeroizing::new(
            STANDARD
                .decode(encoded)
                .map_err(|_| PersonalVmStorageErrorV1::InvalidKey)?,
        );
        let key: Zeroizing<[u8; 32]> = Zeroizing::new(
            decoded
                .as_slice()
                .try_into()
                .map_err(|_| PersonalVmStorageErrorV1::InvalidKey)?,
        );
        Ok(PersonalVmDiskKeyV1(key))
    }

    fn delete_key(&self, owner: &str, profile: &str) -> Result<(), PersonalVmStorageErrorV1> {
        let status = self
            .command("clear", owner, profile)?
            .status()
            .map_err(|_| PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
        status
            .success()
            .then_some(())
            .ok_or(PersonalVmStorageErrorV1::KeyServiceUnavailable)
    }
}
