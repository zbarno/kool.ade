//! Kool.ad/e desktop entry point.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let mut args = std::env::args_os();
    let _binary = args.next();
    let command = args.next();
    if command.as_deref() == Some(std::ffi::OsStr::new("--windows-setup-helper")) {
        let helper_args = std::iter::once(command.expect("helper command was checked")).chain(args);
        match koolade::windows_setup_helper::run_helper(helper_args) {
            Ok(Some(koolade::windows_setup_helper::HelperOutcome::Completed)) => return Ok(()),
            Ok(Some(koolade::windows_setup_helper::HelperOutcome::RebootRequired)) => {
                std::process::exit(3010)
            }
            Ok(Some(koolade::windows_setup_helper::HelperOutcome::Failed)) => std::process::exit(1),
            Ok(None) => unreachable!("helper dispatch rejected its own command"),
            Err(error) => {
                eprintln!("Kool.ad/e Windows setup helper refused the operation: {error:#}");
                std::process::exit(1);
            }
        }
    }
    if command.as_deref() == Some(std::ffi::OsStr::new("--version"))
        || command.as_deref() == Some(std::ffi::OsStr::new("-V"))
    {
        println!("koolade {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if command.as_deref() == Some(std::ffi::OsStr::new("--help"))
        || command.as_deref() == Some(std::ffi::OsStr::new("-h"))
    {
        println!("Kool.ad/e {}", env!("CARGO_PKG_VERSION"));
        println!("\nUsage: koolade [--version | --help | worker serve --stdio]");
        println!("       koolade --internal-mcp-server <config-path>");
        return Ok(());
    }
    if command.as_deref() == Some(std::ffi::OsStr::new("worker")) {
        if args.next().as_deref() != Some(std::ffi::OsStr::new("serve"))
            || args.next().as_deref() != Some(std::ffi::OsStr::new("--stdio"))
            || args.next().is_some()
        {
            eprintln!("Usage: koolade worker serve --stdio");
            std::process::exit(2);
        }
        if let Err(error) = koolade::execution::remote::serve_worker_stdio() {
            eprintln!("Kool.ad/e worker stopped: {error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if command.as_deref() == Some(std::ffi::OsStr::new("--internal-mcp-server")) {
        let Some(config) = args.next() else {
            eprintln!("Kool.ad/e MCP server configuration path is missing.");
            std::process::exit(2);
        };
        if args.next().is_some() {
            eprintln!("Unexpected arguments for the Kool.ad/e MCP server.");
            std::process::exit(2);
        }
        if let Err(error) =
            koolade::harness::serve_internal_mcp_server(std::path::Path::new(&config))
        {
            eprintln!("Kool.ad/e MCP server stopped: {error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    koolade::diagnostics::install();
    let result = eframe::run_native(
        "Kool.ad/e",
        koolade::app::options(),
        Box::new(|_cc: &eframe::CreationContext| {
            Ok(Box::new(koolade::app::KooladeApp::default()) as Box<dyn eframe::App>)
        }),
    );
    if let Err(error) = &result {
        koolade::diagnostics::record_startup_error(&format!("{error:?}"));
    }
    result
}
