use std::{fs, io, path::Path};

/// The real-host acceptance must prove release removed every per-session
/// resource, rather than treating a successful release event as cleanup proof.
pub(super) fn assert_released(
    session_directory: &Path,
    cgroup_directory: &Path,
    overlay_directory: &Path,
) -> Result<(), String> {
    for path in [session_directory, cgroup_directory] {
        match fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(format!(
                    "released session resource remains: {}",
                    path.display()
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("inspect released resource: {error}")),
        }
    }

    let entries = fs::read_dir(overlay_directory)
        .map_err(|error| format!("read released overlay directory: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("read released overlay entry: {error}"))?;
        // Recovery validates and creates the personal overlay root even when
        // this acceptance session is disposable. An empty root owns no session
        // data and is intentionally retained for future personal sessions.
        if entry.file_name() == "personal" {
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| format!("inspect personal overlay root: {error}"))?;
            if metadata.file_type().is_dir()
                && fs::read_dir(entry.path())
                    .map_err(|error| format!("read personal overlay root: {error}"))?
                    .next()
                    .is_none()
            {
                continue;
            }
        }
        return Err(format!(
            "released session overlay remains: {}",
            entry.path().display()
        ));
    }
    Ok(())
}
