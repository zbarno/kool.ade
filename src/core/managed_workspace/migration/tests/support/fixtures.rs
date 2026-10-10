use std::{fs, path::Path};

use crate::domain::{ArtifactIdentity, ChangeMetadata, ChangeStatus};

pub(super) fn feature_markdown(identity: &ArtifactIdentity) -> String {
    let base = format!(
        "# {}: {}\n\n## Intent\n\nCreate planning artifacts.\n",
        identity.display_id, identity.title
    );
    let identified =
        ArtifactIdentity::preserve_markdown(&base, None, &identity.display_id, &identity.title)
            .unwrap();
    let identified = identified.replace(
        &ArtifactIdentity::from_markdown(&identified)
            .unwrap()
            .unwrap()
            .uid,
        &identity.uid,
    );
    ChangeMetadata::rewrite_markdown(&identified, identity, ChangeStatus::Ready, None).unwrap()
}

pub(super) fn identity_markdown(identity: &ArtifactIdentity, body: &str) -> String {
    let marker = serde_json::to_string(identity).unwrap();
    format!("<!-- koolade-artifact-id:v1 {marker} -->\n\n{body}")
}

pub(super) fn walk(root: &Path, path: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            walk(root, &entry.path(), out);
        } else {
            out.push((
                entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                fs::read(entry.path()).unwrap(),
            ));
        }
    }
}
