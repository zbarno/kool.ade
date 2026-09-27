//! Modular product specification paths, validation, and feature activity.

mod active_features;
mod document_updates;
mod feature_index;
pub(crate) mod identity;
mod manifest;
mod paths;
mod schema;

#[cfg(test)]
mod tests;

pub const PRODUCT_DIR: &str = crate::artifacts::layout::canonical::PRODUCT;
pub const INDEX: &str = crate::artifacts::layout::canonical::PRODUCT_INDEX;
pub const LEGACY_ARCHIVE: &str = crate::artifacts::layout::canonical::LEGACY_SPEC_ARCHIVE;
#[cfg(test)]
const OLD_LEGACY_ARCHIVE: &str = crate::artifacts::layout::legacy::SPEC_ARCHIVE;
/// Compatibility paths used only to import the old thirteen-file layout.
pub const LEGACY_MODULES: [&str; 13] = [
    "01-vision.md",
    "02-scope.md",
    "03-actors-and-roles.md",
    "04-feature-inventory.md",
    "05-functional-requirements.md",
    "06-non-functional-requirements.md",
    "07-data-model.md",
    "08-architecture.md",
    "09-environment.md",
    "10-decisions.md",
    "11-risks.md",
    "12-acceptance.md",
    "13-source-map.md",
];
#[cfg(test)]
pub(crate) use active_features::migrate_legacy_change_fixtures;
pub use active_features::{
    active_feature, active_feature_for_workflow, active_features, validate_change_metadata,
};
pub use document_updates::{
    document_path, document_path_for_update, preserved_ids, updated_manifest,
};
pub use feature_index::{next_feature_id, refreshed_index, refreshed_index_from, valid_feature_id};
pub use manifest::{CoreConcept, ProductManifest, ProductModule};
pub use manifest::{legacy_manifest, read as load_manifest};
pub use paths::{safe_module_path, valid_id};
pub use schema::{
    ProductDocument, load_documents, load_modules, module_title, render_product,
    render_product_with_updates, split_legacy, split_sections, validate_module,
};

/// Migration-only test helper; production migration is invoked on connect.
#[cfg(test)]
pub(crate) fn migrate(repo: &std::path::Path, _legacy: &str) -> anyhow::Result<Vec<String>> {
    crate::artifacts::migration::migrate_files_for_test(repo)
}
