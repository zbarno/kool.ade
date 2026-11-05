const APPROVAL_ENV: &str = "KOOLADE_TEST_APPROVE_LOCKED_IMAGES";

mod preparation;
mod process_lock;

pub(crate) use preparation::{prepare_approved_image, prepare_profile_for_home};

pub(crate) fn enabled() -> bool {
    std::env::var(APPROVAL_ENV).ok().as_deref() == Some("1")
}
