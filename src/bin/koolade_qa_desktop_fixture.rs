#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = koolade::runtime_qa::run_linux_desktop_egui_fixture_window() {
        eprintln!("desktop fixture application failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the desktop QA fixture is supported only on Linux");
    std::process::exit(1);
}
