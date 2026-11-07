#[cfg(target_os = "linux")]
fn main() {
    use std::os::unix::ffi::OsStrExt;
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    let bootstrap = arguments
        .next()
        .is_some_and(|argument| argument.as_bytes() == b"--bootstrap-fw-cfg");
    if arguments.next().is_some() {
        eprintln!("computer-use companion received unsupported arguments");
        std::process::exit(2);
    }
    let result = if bootstrap {
        koolade::computer_use::guest_protocol::agent::bootstrap()
    } else {
        koolade::computer_use::guest_protocol::agent::run()
    };
    if result.is_err() {
        eprintln!("computer-use companion stopped because its trusted channel is unavailable");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("computer-use companion is supported only on Linux guests");
    std::process::exit(2);
}
