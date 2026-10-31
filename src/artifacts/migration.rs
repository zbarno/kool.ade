//! Planning-store bootstrapping: create or normalize the product document
//! set inside a managed planning repository.
//!
//! Planning state lives exclusively in the project's separate planning
//! repository. This module bootstraps product documents there; it does not
//! inspect or migrate project artifacts from a code checkout.
mod bootstrap;
mod product;

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
pub(crate) use bootstrap::bootstrap_product;
#[cfg(test)]
pub(crate) use bootstrap::bootstrap_product_from_seed;
pub(crate) use bootstrap::{bootstrap_product_with_store, bootstrap_product_with_store_expected};
