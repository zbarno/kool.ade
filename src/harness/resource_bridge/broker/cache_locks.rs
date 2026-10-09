use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

type CacheGate = Arc<Mutex<()>>;

static CACHE_GATES: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

pub(super) fn gate_for(cache: &Path) -> anyhow::Result<CacheGate> {
    let key = cache.canonicalize()?;
    let gates = CACHE_GATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut gates = gates
        .lock()
        .map_err(|_| anyhow::anyhow!("package cache gate registry is unavailable"))?;
    gates.retain(|_, gate| gate.strong_count() > 0);
    if let Some(gate) = gates.get(&key).and_then(Weak::upgrade) {
        return Ok(gate);
    }
    let gate = Arc::new(Mutex::new(()));
    gates.insert(key, Arc::downgrade(&gate));
    Ok(gate)
}

#[cfg(test)]
mod tests {
    use super::gate_for;
    use std::{fs, sync::Arc};

    #[test]
    fn overlapping_mutations_serialize_per_cache_without_blocking_other_caches() {
        let root =
            std::env::temp_dir().join(format!("koolade-cache-gates-{}", uuid::Uuid::new_v4()));
        let first_cache = root.join("first");
        let second_cache = root.join("second");
        fs::create_dir_all(&first_cache).unwrap();
        fs::create_dir_all(&second_cache).unwrap();

        let first = gate_for(&first_cache).unwrap();
        let same_cache = gate_for(&first_cache).unwrap();
        let other = gate_for(&second_cache).unwrap();
        assert!(Arc::ptr_eq(&first, &same_cache));
        assert!(!Arc::ptr_eq(&first, &other));

        let _first_mutation = first.lock().unwrap();
        assert!(same_cache.try_lock().is_err());
        assert!(other.try_lock().is_ok());

        drop(_first_mutation);
        assert!(same_cache.try_lock().is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
