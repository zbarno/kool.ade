use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(crate) fn validate_changes(changes: &[DocumentationChange]) -> anyhow::Result<()> {
    anyhow::ensure!(
        !changes.is_empty(),
        "no approved documentation was supplied"
    );
    anyhow::ensure!(
        changes.len() <= MAX_DOCUMENT_COUNT,
        "too many documents were supplied for one publication"
    );
    let mut paths = BTreeSet::new();
    let mut total_bytes = 0usize;
    for change in changes {
        validate_document_path(&change.path)?;
        anyhow::ensure!(
            change.content.len() <= MAX_DOCUMENT_BYTES,
            "published document exceeds the 1 MiB limit"
        );
        total_bytes = total_bytes
            .checked_add(change.content.len())
            .ok_or_else(|| anyhow::anyhow!("publication size overflow"))?;
        anyhow::ensure!(
            !change.content.contains('\0') && !change.content.trim().is_empty(),
            "published document is empty or contains a NUL byte"
        );
        anyhow::ensure!(paths.insert(&change.path), "duplicate publication path");
    }
    anyhow::ensure!(
        total_bytes <= MAX_TOTAL_DOCUMENT_BYTES,
        "publication exceeds the 4 MiB total document limit"
    );
    Ok(())
}

fn validate_document_path(path: &str) -> anyhow::Result<()> {
    let valid_prefix = path == "docs/koolade/AGENTS.md"
        || [
            "docs/koolade/product/",
            "docs/koolade/features/",
            "docs/koolade/architecture/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix));
    anyhow::ensure!(
        valid_prefix,
        "publication path is outside Kool.ad/e documentation"
    );
    if path.ends_with("/AGENTS.md") {
        anyhow::ensure!(
            matches!(
                path,
                "docs/koolade/AGENTS.md"
                    | "docs/koolade/product/AGENTS.md"
                    | "docs/koolade/features/AGENTS.md"
                    | "docs/koolade/architecture/AGENTS.md"
            ),
            "only immediate Kool.ad/e documentation indexes may be published"
        );
    }
    anyhow::ensure!(
        path.ends_with(".md")
            && !path.contains('\\')
            && !path.contains('\0')
            && path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
            && path.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.')
            }),
        "publication path is not a safe repository-relative Markdown path"
    );
    Ok(())
}

pub(super) fn digest_changes(changes: &[DocumentationChange]) -> String {
    let mut ordered = changes.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.path.cmp(&right.path));
    let mut digest = Sha256::new();
    for change in ordered {
        digest.update((change.path.len() as u64).to_be_bytes());
        digest.update(change.path.as_bytes());
        digest.update((change.content.len() as u64).to_be_bytes());
        digest.update(change.content.as_bytes());
    }
    format!("{:x}", digest.finalize())
}
