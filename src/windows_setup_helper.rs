//! Narrow, fail-closed entry point for the elevated Windows setup helper.
//!
//! A signed release build must set `KOOLADE_WINDOWS_SETUP_PUBLISHER` to its
//! verified Authenticode publisher. Unsigned builds deliberately cannot run a
//! privileged operation.

use std::ffi::OsString;

const PUBLISHER: Option<&str> = option_env!("KOOLADE_WINDOWS_SETUP_PUBLISHER");

#[cfg(any(windows, test))]
pub(crate) mod protocol;
#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    EnableWslFeatures,
}

/// Typed helper result communicated through the process exit status to the
/// unelevated setup controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperOutcome {
    Completed,
    RebootRequired,
    Failed,
}

impl Operation {
    fn parse(value: &std::ffi::OsStr) -> Option<Self> {
        (value == "enable-wsl-features").then_some(Self::EnableWslFeatures)
    }

    #[cfg(any(windows, test))]
    fn features(self) -> &'static [&'static str] {
        match self {
            Self::EnableWslFeatures => &[
                "Microsoft-Windows-Subsystem-Linux",
                "VirtualMachinePlatform",
            ],
        }
    }
}

#[cfg(any(windows, test))]
fn dism_outcome(exit_code: i32) -> Option<HelperOutcome> {
    match exit_code {
        0 => Some(HelperOutcome::Completed),
        3010 => Some(HelperOutcome::RebootRequired),
        _ => None,
    }
}

pub(crate) fn configured_publisher() -> anyhow::Result<&'static str> {
    PUBLISHER
        .filter(|publisher| !publisher.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("signed Windows setup publisher is not configured"))
}

/// Whether this binary was built with the release publisher trust pin needed
/// to enter the elevated helper path.
pub(crate) fn publisher_trust_configured() -> bool {
    PUBLISHER.is_some_and(|publisher| !publisher.trim().is_empty())
}

/// Dispatch the private helper CLI. It must be called before opening the GUI.
pub fn run_helper(
    args: impl IntoIterator<Item = OsString>,
) -> anyhow::Result<Option<HelperOutcome>> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--windows-setup-helper")) {
        return Ok(None);
    }
    let operation = args
        .next()
        .as_deref()
        .and_then(Operation::parse)
        .ok_or_else(|| anyhow::anyhow!("unsupported Windows setup helper operation"))?;
    anyhow::ensure!(
        args.next().as_deref() == Some(std::ffi::OsStr::new("--handoff-pipe")),
        "an authenticated one-use setup handoff pipe is required"
    );
    let pipe_id = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("setup handoff pipe identity is missing"))?;
    let pipe_id = parse_pipe_id(&pipe_id)?;
    anyhow::ensure!(args.next().is_none(), "unexpected Windows helper arguments");

    #[cfg(windows)]
    {
        windows::run(operation, pipe_id).map(Some)
    }
    #[cfg(not(windows))]
    {
        let _ = (operation, pipe_id);
        anyhow::bail!("Windows setup helper is available only on Windows");
    }
}

fn parse_pipe_id(value: &std::ffi::OsStr) -> anyhow::Result<uuid::Uuid> {
    let pipe_id = value
        .to_str()
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
        .ok_or_else(|| anyhow::anyhow!("setup handoff pipe identity is invalid"))?;
    anyhow::ensure!(
        pipe_id.get_version() == Some(uuid::Version::Random)
            && pipe_id.get_variant() == uuid::Variant::RFC4122,
        "setup handoff pipe identity is invalid"
    );
    Ok(pipe_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn helper_dispatch_is_exact_and_rejects_extra_or_unknown_operations() {
        assert_eq!(run_helper(args(&["--version"])).unwrap(), None);
        assert!(run_helper(args(&["--windows-setup-helper"])).is_err());
        assert!(run_helper(args(&["--windows-setup-helper", "shell"])).is_err());
        assert_eq!(
            Operation::parse(std::ffi::OsStr::new("enable-wsl-features"))
                .unwrap()
                .features(),
            [
                "Microsoft-Windows-Subsystem-Linux",
                "VirtualMachinePlatform"
            ]
        );
        assert_eq!(dism_outcome(0), Some(HelperOutcome::Completed));
        assert_eq!(dism_outcome(3010), Some(HelperOutcome::RebootRequired));
        assert_eq!(dism_outcome(5), None);
        if PUBLISHER.is_none() {
            assert!(configured_publisher().is_err());
        }
        assert!(
            run_helper(args(&[
                "--windows-setup-helper",
                "enable-wsl-features",
                "extra"
            ]))
            .is_err()
        );
        assert!(run_helper(args(&["--windows-setup-helper", "enable-wsl-features"])).is_err());
        assert!(parse_pipe_id(std::ffi::OsStr::new("not-a-pipe")).is_err());
        assert!(
            parse_pipe_id(std::ffi::OsStr::new("00000000-0000-1000-8000-000000000001")).is_err()
        );
        assert!(
            parse_pipe_id(std::ffi::OsStr::new("00000000-0000-4000-0000-000000000001")).is_err()
        );
        assert!(
            parse_pipe_id(std::ffi::OsStr::new("01000000-0000-4000-8000-000000000001")).is_ok()
        );
    }
}
