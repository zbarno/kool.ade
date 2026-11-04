//! Safe local Git command construction shared by code-repository workflows.
mod config;
mod identity;

pub(crate) use config::{
    standalone_git_command, validate_repository_config, validated_git_command,
};
pub(crate) use identity::remote_identity_for;
