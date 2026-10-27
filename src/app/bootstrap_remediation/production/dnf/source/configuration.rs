use std::path::Path;

const CONFIGURATION_DROPIN_DIRS: [(&str, &str); 2] = [
    ("/etc/dnf/libdnf5.conf.d", "/etc/dnf"),
    ("/usr/share/dnf5/libdnf.conf.d", "/usr/share/dnf5"),
];

#[cfg(test)]
#[path = "configuration_tests.rs"]
mod tests;

pub(super) fn inspect_configuration_dropins() -> anyhow::Result<()> {
    for (directory, parent) in CONFIGURATION_DROPIN_DIRS {
        super::verify_root_directory(Path::new(parent))?;
        let path = Path::new(directory);
        match std::fs::symlink_metadata(path) {
            Ok(_) => super::verify_root_directory(path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
        for entry in std::fs::read_dir(path)? {
            let file = entry?.path();
            if file.extension().is_none_or(|extension| extension != "conf") {
                continue;
            }
            let contents = super::read_required_root_file(&file)?;
            inspect_configuration_dropin_contents(&contents)?;
        }
    }
    Ok(())
}

fn inspect_configuration_dropin_contents(contents: &[u8]) -> anyhow::Result<()> {
    super::repositories::inspect_main_configuration(contents)?;
    super::overrides::reject_candidate_repository_sections(contents)
}
