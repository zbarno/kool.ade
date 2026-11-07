mod computer_use_image_promotion {
    mod catalog;
    mod command;

    pub fn run() -> anyhow::Result<()> {
        command::run()
    }
}

use std::process::ExitCode;

fn main() -> ExitCode {
    match computer_use_image_promotion::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("computer-use image promotion failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}
