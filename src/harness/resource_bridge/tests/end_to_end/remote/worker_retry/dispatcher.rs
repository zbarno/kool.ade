use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub(super) fn install_ssh_dispatcher(directory: &Path, workers: &[(&str, &Path)]) -> PathBuf {
    fs::create_dir_all(directory).unwrap();
    let mut script = String::from("#!/bin/sh\ncase \"$*\" in\n");
    for (worker_id, ssh_wrapper) in workers {
        script.push_str(&format!(
            "  *HostKeyAlias={worker_id}*) exec {} \"$@\" ;;\n",
            shell_quote(&ssh_wrapper.to_string_lossy())
        ));
    }
    script
        .push_str("esac\necho 'no SSH fixture matches the pinned worker identity' >&2\nexit 127\n");
    let path = directory.join("ssh");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
