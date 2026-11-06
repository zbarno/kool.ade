use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
};

pub(crate) fn command_text(root: &Path, args: &[&str]) -> Result<String, String> {
    String::from_utf8(command_bytes(root, args)?)
        .map_err(|_| "Git returned non-UTF-8 output".into())
}

pub(crate) fn command_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = sanitized_command(root, args)
        .output()
        .map_err(|_| "Git could not read the selected repository".to_owned())?;
    if !output.status.success() {
        return Err("Git could not read the selected repository revision".into());
    }
    Ok(output.stdout)
}

pub(crate) fn command_bytes_bounded(
    root: &Path,
    args: &[&str],
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut child = sanitized_command(root, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Git could not read the selected repository".to_owned())?;
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Git output stream was unavailable".to_owned())?;
    stdout
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Git output could not be read".to_owned())?;
    if bytes.len() > limit {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Git output exceeded the configured bound".into());
    }
    let status = child
        .wait()
        .map_err(|_| "Git could not finish reading the selected repository".to_owned())?;
    if !status.success() {
        return Err("Git could not read the selected repository revision".into());
    }
    Ok(bytes)
}

fn sanitized_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(root)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_GLOBAL")
        .env_remove("GIT_CONFIG_SYSTEM")
        .env_remove("GIT_CONFIG_NOSYSTEM")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_OPTIONAL_LOCKS");
    command
}

pub(crate) fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
