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

    let mut entries = fs::read_dir(overlay_directory)
        .map_err(|error| format!("read released overlay directory: {error}"))?;
    if let Some(entry) = entries.next() {
        let entry = entry.map_err(|error| format!("read released overlay entry: {error}"))?;
        return Err(format!(
            "released session overlay remains: {}",
            entry.path().display()
        ));
    }
    Ok(())
}
