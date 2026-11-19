mod computer_use_runtime_manifest {
    mod command;

    pub fn run() -> anyhow::Result<()> {
        command::run()
    }
}

use std::process::ExitCode;

fn main() -> ExitCode {
    match computer_use_runtime_manifest::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("computer-use runtime manifest tool failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}
