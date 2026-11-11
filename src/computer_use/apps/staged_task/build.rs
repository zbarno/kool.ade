use sha2::{Digest, Sha256};

use super::super::GuestAppArchitectureV1;
use super::{MAX_STAGED_FILES, MAX_TOTAL_STAGED_BYTES, valid_id, valid_path, valid_tagged_sha256};

/// Private in-memory output from the approved QA build. Bytes are never
/// serialized into QA records, logs, or provider messages.
#[derive(Clone, Eq, PartialEq)]
pub struct StagedTaskBuildFileV1 {
    pub relative_path: String,
    pub bytes: Vec<u8>,
}

/// Bounded build input accepted only after its exact manifest hash matches the
/// persisted Computer QA approval.
#[derive(Clone, Eq, PartialEq)]
pub struct StagedTaskApplicationBuildV1 {
    pub schema_version: u16,
    pub app_id: String,
    pub executable_id: String,
    pub executable_relative_path: String,
    pub app_target_id: String,
    pub execution_recipe_id: String,
    pub build_receipt_sha256: String,
    pub guest_image_digest: String,
    pub guest_architecture: GuestAppArchitectureV1,
    pub files: Vec<StagedTaskBuildFileV1>,
}

impl StagedTaskApplicationBuildV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1
            || self.app_id != crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1
            || !valid_id(&self.executable_id)
            || !valid_path(&self.executable_relative_path)
            || !valid_id(&self.app_target_id)
            || !valid_id(&self.execution_recipe_id)
            || !valid_tagged_sha256(&self.guest_image_digest)
            || !valid_bare_sha256(&self.build_receipt_sha256)
            || self.files.is_empty()
            || self.files.len() > MAX_STAGED_FILES
        {
            return Err("staged task build identity or bounds are invalid");
        }
        let mut previous_path: Option<&str> = None;
        let mut total_bytes = 0_u64;
        let mut executable_found = false;
        let mut manifest = Vec::with_capacity(self.files.len());
        for file in &self.files {
            if !valid_path(&file.relative_path)
                || file.bytes.is_empty()
                || file.bytes.len() as u64
                    > crate::computer_use::artifacts::COMPUTER_ARTIFACT_MAX_BYTES_V1
                || previous_path.is_some_and(|previous| previous >= file.relative_path.as_str())
            {
                return Err("staged task build files are invalid, unsorted, or exceed bounds");
            }
            executable_found |= file.relative_path == self.executable_relative_path;
            total_bytes = total_bytes.saturating_add(file.bytes.len() as u64);
            if total_bytes > MAX_TOTAL_STAGED_BYTES {
                return Err("staged task build exceeds the aggregate transfer bound");
            }
            let digest = format!("{:x}", Sha256::digest(&file.bytes));
            manifest.push((&file.relative_path, digest));
            previous_path = Some(&file.relative_path);
        }
        let canonical = serde_json::to_vec(&manifest)
            .map_err(|_| "staged task build manifest could not be encoded")?;
        let digest = format!("{:x}", Sha256::digest(canonical));
        if !executable_found || digest != self.build_receipt_sha256 {
            return Err("staged task build files do not match the approved receipt");
        }
        Ok(())
    }
}

fn valid_bare_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> StagedTaskApplicationBuildV1 {
        let files = vec![
            StagedTaskBuildFileV1 {
                relative_path: "bin/editor".into(),
                bytes: b"elf fixture".to_vec(),
            },
            StagedTaskBuildFileV1 {
                relative_path: "config/app.json".into(),
                bytes: b"{}".to_vec(),
            },
        ];
        let manifest = files
            .iter()
            .map(|file| {
                (
                    file.relative_path.as_str(),
                    format!("{:x}", Sha256::digest(&file.bytes)),
                )
            })
            .collect::<Vec<_>>();
        let canonical = serde_json::to_vec(&manifest).unwrap();
        StagedTaskApplicationBuildV1 {
            schema_version: 1,
            app_id: crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into(),
            executable_id: "editor".into(),
            executable_relative_path: "bin/editor".into(),
            app_target_id: "target-1".into(),
            execution_recipe_id: "recipe-1".into(),
            build_receipt_sha256: format!("{:x}", Sha256::digest(canonical)),
            guest_image_digest: format!("sha256:{}", "a".repeat(64)),
            guest_architecture: GuestAppArchitectureV1::X86_64,
            files,
        }
    }

    #[test]
    fn in_memory_task_build_requires_exact_sorted_manifest_content() {
        let build = build();
        assert_eq!(build.validate(), Ok(()));

        let mut tampered = build.clone();
        tampered.files[0].bytes[0] ^= 1;
        assert_eq!(
            tampered.validate(),
            Err("staged task build files do not match the approved receipt")
        );

        let mut unsorted = build;
        unsorted.files.swap(0, 1);
        assert_eq!(
            unsorted.validate(),
            Err("staged task build files are invalid, unsorted, or exceed bounds")
        );
    }
}
