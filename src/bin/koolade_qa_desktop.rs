use std::{fs, io::Write, process::ExitCode};

const REQUEST_PATH: &str = "/run/koolade-runtime-qa/request.json";

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("desktop QA driver failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("--fixture-smoke"), None) => {
            let response = koolade::runtime_qa::smoke_linux_desktop_fixture()?;
            serde_json::to_writer(std::io::stdout().lock(), &response)
                .map_err(|_| "desktop fixture smoke response could not encode".to_owned())?;
            std::io::stdout()
                .lock()
                .write_all(b"\n")
                .map_err(|_| "desktop fixture smoke response could not write".to_owned())
        }
        (Some("--fixture-window"), None) => koolade::runtime_qa::run_linux_desktop_fixture_window(),
        (Some("--fixture-egui-window"), None) => {
            koolade::runtime_qa::run_linux_desktop_egui_fixture_window()
        }
        (Some("--smoke"), None) => {
            let (width, height, screenshot_bytes) =
                koolade::runtime_qa::smoke_linux_desktop_driver()?;
            let response = serde_json::json!({
                "schema_version": 1,
                "status": "ready",
                "display_width": width,
                "display_height": height,
                "screenshot_bytes": screenshot_bytes,
            });
            serde_json::to_writer(std::io::stdout().lock(), &response)
                .map_err(|_| "desktop QA smoke response could not encode".to_owned())?;
            std::io::stdout()
                .lock()
                .write_all(b"\n")
                .map_err(|_| "desktop QA smoke response could not write".to_owned())
        }
        (None, None) => {
            let bytes = fs::read(REQUEST_PATH)
                .map_err(|_| "app-owned desktop QA request file is unavailable".to_owned())?;
            let response = koolade::runtime_qa::execute_linux_desktop_driver_request(&bytes)?;
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(&response)
                .and_then(|()| stdout.write_all(b"\n"))
                .map_err(|_| "desktop QA response could not write".to_owned())
        }
        _ => Err(
            "desktop QA driver accepts only the fixed request path or a fixed smoke mode".into(),
        ),
    }
}
