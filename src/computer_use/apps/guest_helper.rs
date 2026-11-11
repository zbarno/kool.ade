mod client;
mod config;
mod execute;
mod protocol;
mod server;

pub(crate) use client::{
    StagedTaskClientV1, app_management_available, installed_app_ids, launch, launch_staged_task,
    manage, staged_task_app_launch_available,
};
pub use config::GuestAppHelperConfigV1;

pub fn run_guest_app_helper() -> Result<(), &'static str> {
    server::run()
}
