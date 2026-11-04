use super::Implementation;
use std::{fs, path::Path};

/// Load implementation state written by the current task repository format.
pub(crate) fn read_state_file(path: &Path) -> anyhow::Result<Implementation> {
    let bytes = fs::read(path)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        value
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            == Some(2),
        "Unsupported implementation state version"
    );
    Ok(serde_json::from_value(value)?)
}

pub(crate) fn save(dir: &Path, state: &Implementation) -> anyhow::Result<()> {
    crate::artifacts::atomic_write_bytes(&dir.join("state.json"), &serialize_state(state)?)
}

pub(crate) fn serialize_state(state: &Implementation) -> anyhow::Result<Vec<u8>> {
    let mut value = serde_json::to_value(state)?;
    value
        .as_object_mut()
        .expect("Implementation serializes as an object")
        .insert("schemaVersion".into(), serde_json::json!(2));
    Ok(serde_json::to_vec_pretty(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_state(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "koolade-implementation-state-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn only_current_version_state_is_loaded() {
        let path = temporary_state("current", br#"{"schemaVersion":2}"#);
        assert!(read_state_file(&path).is_err());
        fs::write(&path, br#"{"schemaVersion":1}"#).unwrap();
        assert!(read_state_file(&path).is_err());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
