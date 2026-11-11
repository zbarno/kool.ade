#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = koolade::computer_use::apps::guest_helper::run_guest_app_helper() {
        eprintln!("computer-use app helper stopped: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("computer-use app helper is supported only on Linux guests");
    std::process::exit(2);
}
