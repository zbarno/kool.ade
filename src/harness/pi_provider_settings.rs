//! Read configured Pi model identifiers for the provider settings screen.
use std::{io::Read, path::PathBuf};

pub(crate) fn configured_model_catalog() -> anyhow::Result<(Vec<String>, Option<String>)> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Cannot find Pi home for model discovery"))?;
    let agent = home.join(".pi/agent");
    let settings: serde_json::Value = read_json(&agent.join("settings.json"))?;
    let models: serde_json::Value = read_json(&agent.join("models.json"))?;
    let provider = settings["defaultProvider"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Pi has no default model provider"))?;
    let entries = models["providers"][provider]["models"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Pi's configured provider has no model catalog"))?;
    let mut ids = entries
        .iter()
        .filter_map(|model| model["id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok((ids, settings["defaultModel"].as_str().map(str::to_owned)))
}

fn read_json(path: &std::path::Path) -> anyhow::Result<serde_json::Value> {
    const LIMIT: u64 = 1024 * 1024;
    let before_open = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        before_open.is_file()
            && !before_open.file_type().is_symlink()
            && before_open.len() <= LIMIT,
        "Pi provider settings file is not a bounded regular file"
    );
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= LIMIT,
        "Pi provider settings file is not a bounded regular file"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= LIMIT,
        "Pi provider settings file exceeds the size limit"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
