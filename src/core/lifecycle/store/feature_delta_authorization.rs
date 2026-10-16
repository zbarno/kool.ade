use crate::{artifacts::planning_store::StoreError, core::lifecycle::FeatureDeltaAuthorizationV1};

use super::LifecycleStore;

impl LifecycleStore<'_> {
    pub fn feature_delta_authorization_refs(
        &self,
        feature_uid: &str,
    ) -> Result<std::collections::HashSet<String>, StoreError> {
        let mut receipts = std::collections::HashSet::new();
        for path in self
            .store
            .list_record_paths("state/lifecycle_feature_delta_authorizations")?
        {
            let receipt_uid = path
                .strip_prefix("state/lifecycle_feature_delta_authorizations/")
                .and_then(|path| path.strip_suffix(".json"))
                .ok_or_else(|| {
                    StoreError::MalformedState("feature delta authorization path is invalid".into())
                })?;
            let authorization = self.load_feature_delta_authorization(receipt_uid)?;
            if authorization.feature_uid == feature_uid {
                receipts.insert(authorization.receipt_uid);
            }
        }
        Ok(receipts)
    }

    pub fn load_feature_delta_authorization(
        &self,
        receipt_uid: &str,
    ) -> Result<FeatureDeltaAuthorizationV1, StoreError> {
        if !crate::core::lifecycle::types::is_uuid(receipt_uid) {
            return Err(StoreError::MalformedState(
                "feature delta authorization identity is invalid".into(),
            ));
        }
        let path = format!("state/lifecycle_feature_delta_authorizations/{receipt_uid}.json");
        let bytes = self.store.read(&path)?;
        let authorization: FeatureDeltaAuthorizationV1 = serde_json::from_slice(&bytes)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        authorization
            .validate()
            .map_err(StoreError::MalformedState)?;
        if authorization.receipt_uid != receipt_uid {
            return Err(StoreError::MalformedState(
                "feature delta authorization identity does not match its path".into(),
            ));
        }
        Ok(authorization)
    }
}
