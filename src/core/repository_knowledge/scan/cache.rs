use serde::{Deserialize, Serialize};

use crate::artifacts::planning_store::PlanningStore;
use crate::core::repository_knowledge::{RepositoryKnowledgeProfileV1, RepositoryRevisionKeyV1};

const CACHE_PATH: &str = "knowledge/repository-profiles.json";
const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_CACHE_PROFILES: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CacheEnvelope {
    schema_version: u32,
    profiles: Vec<RepositoryKnowledgeProfileV1>,
}

pub(super) fn cached(
    store: &PlanningStore,
    key: &RepositoryRevisionKeyV1,
) -> Option<RepositoryKnowledgeProfileV1> {
    let size = store
        .list_files("knowledge")
        .ok()?
        .into_iter()
        .find(|file| file.name == "repository-profiles.json")?
        .bytes;
    if size > MAX_CACHE_BYTES as u64 {
        return None;
    }
    let bytes = store.read(CACHE_PATH).ok()?;
    let cache: CacheEnvelope = serde_json::from_slice(&bytes).ok()?;
    if cache.schema_version != 1 || cache.profiles.len() > MAX_CACHE_PROFILES {
        return None;
    }
    cache
        .profiles
        .into_iter()
        .find(|profile| profile.key == *key && profile.validate().is_ok())
}

pub(super) fn save_cache(store: &PlanningStore, profile: RepositoryKnowledgeProfileV1) {
    let mut cache = read_cache(store);
    cache
        .profiles
        .retain(|previous| previous.key != profile.key);
    cache.profiles.push(profile);
    cache.profiles.sort_by_key(|entry| entry.scanned_at_unix_ms);
    while cache.profiles.len() > MAX_CACHE_PROFILES {
        cache.profiles.remove(0);
    }
    loop {
        let Ok(bytes) = serde_json::to_vec(&cache) else {
            return;
        };
        if bytes.len() <= MAX_CACHE_BYTES {
            let _ = store.atomic_write(CACHE_PATH, &bytes);
            return;
        }
        if cache.profiles.is_empty() {
            return;
        }
        cache.profiles.remove(0);
    }
}

fn read_cache(store: &PlanningStore) -> CacheEnvelope {
    let Ok(files) = store.list_files("knowledge") else {
        return empty_cache();
    };
    let Some(file) = files
        .iter()
        .find(|file| file.name == "repository-profiles.json")
    else {
        return empty_cache();
    };
    if file.bytes > MAX_CACHE_BYTES as u64 {
        return empty_cache();
    }
    store
        .read(CACHE_PATH)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CacheEnvelope>(&bytes).ok())
        .filter(|cache| cache.schema_version == 1 && cache.profiles.len() <= MAX_CACHE_PROFILES)
        .unwrap_or_else(empty_cache)
}

fn empty_cache() -> CacheEnvelope {
    CacheEnvelope {
        schema_version: 1,
        profiles: Vec::new(),
    }
}
