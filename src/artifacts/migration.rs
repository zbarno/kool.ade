//! Planning-store bootstrapping: create or normalize the product document
//! set inside a managed planning repository.
//!
//! Legacy in-place repository migrations no longer run on connect. Planning
//! state lives exclusively in the project's separate planning repository;
//! moving an old embedded checkout into that model is the explicit,
//! one-way managed-workspace migration.
mod bootstrap;
mod product;

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
pub(crate) use bootstrap::bootstrap_product;
#[cfg(test)]
pub(crate) use bootstrap::bootstrap_product_from_seed;
pub(crate) use bootstrap::{bootstrap_product_with_store, bootstrap_product_with_store_expected};
