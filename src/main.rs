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
        println!("       koolade computer-use-worker serve --stdio --config <private-config>");
        println!("       koolade computer-use-target enroll --config <enrollment.json>");
        println!("       koolade computer-use-target select --worker-id <id>");
        println!(
            "       koolade computer-use-target recover --worker-id <id> --session-id <uuid> --approve"
        );
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
    if command.as_deref() == Some(std::ffi::OsStr::new("computer-use-target")) {
        let Some(subcommand) = args.next() else {
            eprintln!(
                "Usage: koolade computer-use-target enroll --config <enrollment.json> | select --worker-id <id>"
            );
            std::process::exit(2);
        };
        let result = if subcommand == std::ffi::OsStr::new("recover") {
            let mut worker_id: Option<std::ffi::OsString> = None;
            let mut session_id: Option<std::ffi::OsString> = None;
            let mut approve = false;
            while let Some(argument) = args.next() {
                if argument == std::ffi::OsStr::new("--worker-id") && worker_id.is_none() {
                    worker_id = args.next();
                } else if argument == std::ffi::OsStr::new("--session-id") && session_id.is_none() {
                    session_id = args.next();
                } else if argument == std::ffi::OsStr::new("--approve") && !approve {
                    approve = true;
                } else {
                    eprintln!(
                        "Usage: koolade computer-use-target recover --worker-id <id> --session-id <uuid> --approve"
                    );
                    std::process::exit(2);
                }
            }
            let (Some(worker_id), Some(session_id)) = (worker_id, session_id) else {
                eprintln!(
                    "Usage: koolade computer-use-target recover --worker-id <id> --session-id <uuid> --approve"
                );
                std::process::exit(2);
            };
            (|| -> anyhow::Result<()> {
                let worker_id = worker_id
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("worker ID is not UTF-8"))?;
                let session_id = session_id
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("session ID is not UTF-8"))?;
                let action_unknown =
                    koolade::computer_use::remote_worker::recover_computer_use_worker_session_v1(
                        worker_id, session_id, approve,
                    )?;
                if action_unknown {
                    println!(
                        "Worker session transport recovered. A prior guest action remains outcome-unknown and was not replayed; release or inspect it before further input."
                    );
                } else {
                    println!("Worker session transport recovered with a new signed lease.");
                }
                Ok(())
            })()
        } else if subcommand == std::ffi::OsStr::new("enroll")
            && args.next().as_deref() == Some(std::ffi::OsStr::new("--config"))
        {
            let Some(path) = args.next() else {
                eprintln!("Computer Use enrollment config path is missing.");
                std::process::exit(2);
            };
            if args.next().is_some() {
                eprintln!("Unexpected Computer Use enrollment arguments.");
                std::process::exit(2);
            }
            (|| -> anyhow::Result<()> {
                let metadata = std::fs::symlink_metadata(&path)?;
                anyhow::ensure!(
                    metadata.is_file()
                        && !metadata.file_type().is_symlink()
                        && metadata.len() <= 32 * 1024,
                    "enrollment config is unsafe or too large"
                );
                let bytes = std::fs::read(path)?;
                let enrollment: koolade::computer_use::remote_worker::ComputerRemoteWorkerEnrollmentV1 = serde_json::from_slice(&bytes)?;
                let worker_id = enrollment.worker_id.clone();
                koolade::persistence::computer_use_remote_workers::enroll(enrollment)?;
                println!("Enrolled Computer Use worker {worker_id}.");
                Ok(())
            })()
        } else if subcommand == std::ffi::OsStr::new("select")
            && args.next().as_deref() == Some(std::ffi::OsStr::new("--worker-id"))
        {
            let Some(worker_id) = args.next() else {
                eprintln!("Computer Use worker ID is missing.");
                std::process::exit(2);
            };
            if args.next().is_some() {
                eprintln!("Unexpected Computer Use selection arguments.");
                std::process::exit(2);
            }
            (|| -> anyhow::Result<()> {
                let worker_id = worker_id
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("worker ID is not UTF-8"))?;
                koolade::persistence::computer_use_remote_workers::select(worker_id)?;
                println!(
                    "Selected Computer Use worker {worker_id}. The remote worker release gate remains closed until independent certification."
                );
                Ok(())
            })()
        } else {
            eprintln!(
                "Usage: koolade computer-use-target enroll --config <enrollment.json> | select --worker-id <id>"
            );
            std::process::exit(2);
        };
        if let Err(error) = result {
            eprintln!("Computer Use target operation failed: {error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if command.as_deref() == Some(std::ffi::OsStr::new("computer-use-worker")) {
        if args.next().as_deref() != Some(std::ffi::OsStr::new("serve"))
            || args.next().as_deref() != Some(std::ffi::OsStr::new("--stdio"))
            || args.next().as_deref() != Some(std::ffi::OsStr::new("--config"))
        {
            eprintln!("Usage: koolade computer-use-worker serve --stdio --config <private-config>");
            std::process::exit(2);
        }
        let Some(config) = args.next() else {
            eprintln!("Computer Use worker config path is missing.");
            std::process::exit(2);
        };
        if args.next().is_some() {
            eprintln!("Unexpected Computer Use worker arguments.");
            std::process::exit(2);
        }
        #[cfg(target_os = "linux")]
        if let Err(error) = koolade::computer_use::remote_worker::serve_computer_use_worker_stdio_v1(
            std::path::Path::new(&config),
        ) {
            eprintln!("Kool.ad/e Computer Use worker stopped: {error:#}");
            std::process::exit(1);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            eprintln!("Remote Computer Use workers require Linux with supported KVM isolation.");
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
