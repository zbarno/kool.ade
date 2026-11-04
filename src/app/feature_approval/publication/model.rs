#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app::root::feature_approval) struct PendingFeaturePublication {
    pub(in crate::app::root::feature_approval) schema_version: u32,
    pub(in crate::app::root::feature_approval) feature_id: String,
    pub(in crate::app::root::feature_approval) feature_uid: String,
    pub(in crate::app::root::feature_approval) feature_snapshot_sha256: String,
    pub(in crate::app::root::feature_approval) feature_markdown_sha256: String,
    pub(in crate::app::root::feature_approval) published_pin:
        crate::core::published_specs::PublishedSpecPin,
    pub(in crate::app::root::feature_approval) consent_sha256: String,
    pub(in crate::app::root::feature_approval) changes:
        Vec<crate::core::spec_publication::DocumentationChange>,
}

#[cfg_attr(test, allow(dead_code))]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app::root::feature_approval) struct FeaturePublicationReceipt {
    pub(in crate::app::root::feature_approval) schema_version: u32,
    pub(in crate::app::root::feature_approval) pull_request_url: String,
    pub(in crate::app::root::feature_approval) branch: String,
    pub(in crate::app::root::feature_approval) base_commit: String,
    pub(in crate::app::root::feature_approval) head_commit: String,
}

pub(in crate::app::root::feature_approval) fn paths(uid: &str, snapshot: &str) -> (String, String) {
    (
        format!("state/feature_publications/{uid}-{snapshot}.json"),
        format!("state/feature_publications/pending/{uid}-{snapshot}.json"),
    )
}

pub(in crate::app::root::feature_approval) fn document_path(uid: &str) -> String {
    format!("docs/koolade/features/{uid}.md")
}
