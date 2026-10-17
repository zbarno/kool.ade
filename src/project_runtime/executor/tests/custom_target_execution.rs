// These checks cross the persisted manifest, local approval store, validated
// ExecutionSession, and backend dispatch seam. Validator tests cannot prove
// forged shared approval remains denied at dispatch. The backend is an
// in-memory control-plane double: no Docker daemon, SSH, network, or host CLI;
// setup cost is low and fully in-process.
#[path = "custom_target_execution/approved.rs"]
mod approved;
#[path = "custom_target_execution/backend.rs"]
mod backend;
#[path = "custom_target_execution/forged_shared.rs"]
mod forged_shared;
#[path = "custom_target_execution/mixed_target.rs"]
mod mixed_target;
#[path = "custom_target_execution/security.rs"]
mod security;
#[path = "custom_target_execution/setup.rs"]
mod setup;
