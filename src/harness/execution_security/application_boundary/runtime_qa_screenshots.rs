use sha2::Digest;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::super::mcp_server::ServerConfig;
use super::PrivateDirectory;
use crate::harness::ReviewExecutionIdentity;

const MAX_REVIEW_IMAGES: usize = 2_048;
const MAX_REVIEW_BYTES: usize = 64 * 1024 * 1024;

pub(super) struct PreparedReviewImages {
    pub config: ServerConfig,
    pub audit_root: PathBuf,
    pub inspected: Arc<Mutex<BTreeSet<String>>>,
    pub(super) allowed_ids: BTreeSet<String>,
}

impl PreparedReviewImages {
    pub fn new(
        scratch: &PrivateDirectory,
        identity: &ReviewExecutionIdentity,
        task_id: &str,
        workspace_id: &str,
        source_sha: &str,
    ) -> anyhow::Result<Self> {
        let review = identity
            .runtime_qa_screenshot_review
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("runtime QA visual review payload is unavailable"))?;
        anyhow::ensure!(
            review.screenshots.len() <= MAX_REVIEW_IMAGES,
            "runtime QA visual review screenshot set is too large"
        );
        let image_root = scratch.create_directory("qa-screenshots")?;
        let audit_root = scratch.create_directory("qa-screenshot-views")?;
        let mut hashes = std::collections::BTreeMap::new();
        let mut allowed_ids = BTreeSet::new();
        let mut total_bytes = 0usize;
        for screenshot in &review.screenshots {
            anyhow::ensure!(
                uuid::Uuid::parse_str(&screenshot.id)
                    .is_ok_and(|parsed| parsed.hyphenated().to_string() == screenshot.id)
                    && allowed_ids.insert(screenshot.id.clone()),
                "runtime QA visual review has an invalid or duplicate screenshot ID"
            );
            anyhow::ensure!(
                format!("{:x}", sha2::Sha256::digest(&screenshot.bytes)) == screenshot.sha256
                    && screenshot.bytes.len() <= 16 * 1024 * 1024
                    && screenshot.bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                    && image::load_from_memory(&screenshot.bytes).is_ok(),
                "runtime QA visual review screenshot failed hash or PNG validation"
            );
            total_bytes = total_bytes.saturating_add(screenshot.bytes.len());
            anyhow::ensure!(
                total_bytes <= MAX_REVIEW_BYTES,
                "runtime QA visual review screenshot set exceeds its review quota"
            );
            scratch.write_file(
                &format!("qa-screenshots/{}.png", screenshot.id),
                &screenshot.bytes,
            )?;
            hashes.insert(screenshot.id.clone(), screenshot.sha256.clone());
        }
        let config = ServerConfig::runtime_qa_screenshot_review(
            identity.session_id.clone(),
            task_id.to_owned(),
            format!("{workspace_id}:{task_id}:{source_sha}"),
            identity.execution_id.clone(),
            image_root,
            audit_root.clone(),
            hashes,
        );
        Ok(Self {
            config,
            audit_root,
            inspected: Arc::clone(&review.inspected_screenshot_ids),
            allowed_ids,
        })
    }
}

pub(super) fn merge_inspected(
    audit_root: &Path,
    allowed_ids: &BTreeSet<String>,
    sink: &Arc<Mutex<BTreeSet<String>>>,
) {
    let Ok(entries) = std::fs::read_dir(audit_root) else {
        return;
    };
    let observed = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let id = name.to_str()?.strip_suffix(".viewed")?;
            let metadata = std::fs::symlink_metadata(entry.path()).ok()?;
            (metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() == 0
                && allowed_ids.contains(id))
            .then(|| id.to_owned())
        })
        .collect::<Vec<_>>();
    if let Ok(mut sink) = sink.lock() {
        sink.extend(observed);
    }
}
