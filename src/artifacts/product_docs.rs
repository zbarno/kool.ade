//! Modular product specification paths, validation, and feature activity.

mod active_features;
mod document_updates;
mod feature_index;
pub(crate) mod identity;
#[cfg(test)]
mod manifest;
mod paths;
mod schema;

#[cfg(test)]
mod index_stickiness;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub const PRODUCT_DIR: &str = crate::artifacts::layout::store::PRODUCT;
#[cfg(test)]
pub const INDEX: &str = crate::artifacts::layout::store::PRODUCT_INDEX;
pub use active_features::{
    active_feature, active_feature_for_workflow, active_feature_for_workflow_in, active_features,
    published_features, validate_change_metadata,
};
#[cfg(test)]
pub use document_updates::updated_manifest;
pub use document_updates::{
    current_document, document_path, document_path_for_update, next_feature_id_for_update,
    preserved_ids,
};
pub use feature_index::{next_feature_id, valid_feature_id};
#[cfg(test)]
pub use feature_index::{refreshed_index, refreshed_index_from};
#[cfg(test)]
pub use manifest::{ProductManifest, read as load_manifest};
pub use paths::{safe_module_path, valid_id};
pub(crate) use schema::load_published_product;
pub use schema::{
    ProductDocument, load_documents, load_modules, module_title, render_product,
    render_product_with_updates, render_published_product, render_published_product_with_updates,
    split_sections, validate_module,
};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum CoreConcept {
    Overview,
    UsersAndOutcomes,
    CurrentCapabilities,
    ArchitectureAndConstraints,
    Decisions,
    QualityAndAcceptance,
}

impl CoreConcept {
    pub fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::UsersAndOutcomes => "users-and-outcomes",
            Self::CurrentCapabilities => "current-capabilities",
            Self::ArchitectureAndConstraints => "architecture-and-constraints",
            Self::Decisions => "decisions",
            Self::QualityAndAcceptance => "quality-and-acceptance",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::UsersAndOutcomes => "Users and Outcomes",
            Self::CurrentCapabilities => "Current Capabilities",
            Self::ArchitectureAndConstraints => "Architecture and Constraints",
            Self::Decisions => "Decisions",
            Self::QualityAndAcceptance => "Quality and Acceptance",
        }
    }

    pub fn all() -> [Self; 6] {
        [
            Self::Overview,
            Self::UsersAndOutcomes,
            Self::CurrentCapabilities,
            Self::ArchitectureAndConstraints,
            Self::Decisions,
            Self::QualityAndAcceptance,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductModule {
    pub uid: String,
    pub id: String,
    pub title: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_concept: Option<CoreConcept>,
}

impl ProductModule {
    pub fn new(id: &str, title: &str, core_concept: Option<CoreConcept>) -> Self {
        Self {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            id: id.to_owned(),
            title: title.to_owned(),
            path: format!("{id}.md"),
            core_concept,
        }
    }
}
