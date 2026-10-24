//! Consent-gated preparation of the bundled general runtime image.

mod controller;
mod executor;
#[cfg(test)]
#[path = "bootstrap_image_prepare/tests.rs"]
mod tests;

pub(crate) use controller::Controller;
