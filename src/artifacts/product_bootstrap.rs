//! Create the current product-document skeleton in application-owned storage.
#[cfg(test)]
mod bootstrap;
#[cfg(test)]
mod product;

#[cfg(test)]
pub(crate) use bootstrap::bootstrap_product;
#[cfg(test)]
pub(crate) use bootstrap::bootstrap_product_with_store;
