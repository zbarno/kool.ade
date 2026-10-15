use super::*;
use crate::harness::{
    DependencyDecision, DependencyKind, DependencyNeed, DependencyRequestStatus, PackageEcosystem,
};
mod addition;
mod cargo;
mod npm_malicious;
mod remote;
mod support;

use support::{commit, initialize_project, npm_available};
