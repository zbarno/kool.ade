//! Sanitized, build-owned product capability catalog and bounded lookup.
mod catalog;

pub use catalog::{
    CapabilityAnswer, CapabilityMatch, CapabilityState, CapabilityStatus,
    ProductCapabilityCatalogV1, ReadinessEvidence, ReadinessState,
};
