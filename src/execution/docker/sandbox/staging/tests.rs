use super::{capture, capture_reviewed, capture_scoped, synchronize};
pub(super) use crate::execution::docker::sandbox::ValidatedWorkspace;
pub(super) use std::{fs, io::Cursor, path::Path};
pub(super) use tar::{Builder, Header};

#[path = "tests/support.rs"]
mod support;
use support::{
    Fixture, git, returned, returned_with_hard_link, returned_with_mode, traversal_returned,
};

#[path = "tests/archive.rs"]
mod archive_tests;
#[path = "tests/artifacts.rs"]
mod artifacts;
#[cfg(unix)]
#[path = "tests/capture.rs"]
mod capture_tests;
#[path = "tests/reviewed_capture.rs"]
mod reviewed_capture;
#[cfg(unix)]
#[path = "tests/scoped.rs"]
mod scoped;
#[path = "tests/scoped_failure.rs"]
mod scoped_failure;
#[cfg(unix)]
#[path = "tests/scoped_recovery.rs"]
mod scoped_recovery;
#[cfg(unix)]
#[path = "tests/synchronization.rs"]
mod synchronization;
