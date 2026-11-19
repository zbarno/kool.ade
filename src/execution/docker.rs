//! Trusted Docker Engine discovery, image selection, and bounded execution.

pub mod backend;
pub mod discovery;
pub mod engine;
pub mod environment;
pub(crate) mod image_extension;
pub mod images;
pub mod prerequisites;
mod private_storage;
pub mod provider_preflight;
pub mod provider_session;
pub mod qa_container;
pub mod sandbox;

#[cfg(test)]
#[path = "docker/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "docker/image_profile_tests.rs"]
mod image_profile_tests;
