mod input;
mod scope;

pub(super) use input::build_request;
pub(super) use scope::{collect_scope, validate_included_tasks};

const CHUNK_BYTES: usize = 8 * 1024;
