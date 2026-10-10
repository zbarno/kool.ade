use super::{PlanningStore, RecordRevisionCheck, StoreError};

#[cfg(test)]
#[path = "record_api/tests.rs"]
mod tests;

impl PlanningStore {
    /// Read and validate one normalized record, returning its current revision.
    pub fn read_record<T: serde::de::DeserializeOwned>(
        &self,
        relative: &str,
    ) -> Result<(T, u64), StoreError> {
        self.with_consistent_read(|| self.read_record_unlocked(relative))
    }

    fn read_record_unlocked<T: serde::de::DeserializeOwned>(
        &self,
        relative: &str,
    ) -> Result<(T, u64), StoreError> {
        let revision = super::records::record_revision(self, relative)?;
        let bytes = self.read(relative)?;
        let record = serde_json::from_slice(&bytes).map_err(|error| {
            StoreError::MalformedState(format!("record {relative} cannot be decoded: {error}"))
        })?;
        Ok((record, revision))
    }

    /// List normalized record paths in one collection, rejecting malformed
    /// names and symlinks rather than silently omitting them.
    pub fn list_record_paths(&self, collection: &str) -> Result<Vec<String>, StoreError> {
        self.with_consistent_read(|| self.list_record_paths_unlocked(collection))
    }

    fn list_record_paths_unlocked(&self, collection: &str) -> Result<Vec<String>, StoreError> {
        super::records::validate_record_collection(collection)?;
        self.list_files(collection)?
            .into_iter()
            .map(|file| {
                let stem = file.name.strip_suffix(".json").ok_or_else(|| {
                    StoreError::MalformedState(format!(
                        "unexpected file in {collection}: {}",
                        file.name
                    ))
                })?;
                let uid = uuid::Uuid::parse_str(stem).map_err(|_| {
                    StoreError::MalformedState(format!(
                        "invalid record filename in {collection}: {}",
                        file.name
                    ))
                })?;
                let canonical = uid.hyphenated().to_string();
                if canonical != stem {
                    return Err(StoreError::MalformedState(format!(
                        "noncanonical record filename in {collection}: {}",
                        file.name
                    )));
                }
                let path = format!("{collection}/{canonical}.json");
                super::records::validate_record_path(&path)?;
                Ok(path)
            })
            .collect()
    }

    /// Save one normalized record with an optimistic revision check.
    pub fn save_record<T: serde::Serialize>(
        &self,
        relative: &str,
        record: &T,
        expected_revision: u64,
    ) -> Result<(Vec<String>, String), StoreError> {
        super::records::validate_record_path(relative)?;
        let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
            StoreError::MalformedState(format!("record {relative} cannot be encoded: {error}"))
        })?;
        self.transaction_with_record_revisions(
            &[(relative.to_owned(), bytes)],
            &[RecordRevisionCheck {
                path: relative.to_owned(),
                expected_revision,
            }],
        )
    }
}
