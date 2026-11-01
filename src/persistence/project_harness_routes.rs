//! Shared per-project role routes stored with the project's planning artifacts.

use crate::{
    artifacts::planning_store::{PlanningStore, StoreError, paths},
    persistence::harness_settings::{MANAGER, TASK_ROUTABLE_CATEGORIES, WorkRoute},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const SCHEMA_VERSION: u8 = 1;
const MAX_ROUTE_FILE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectHarnessRoutes {
    pub schema_version: u8,
    pub routes: BTreeMap<String, WorkRoute>,
}

impl ProjectHarnessRoutes {
    pub fn with_routes(routes: BTreeMap<String, WorkRoute>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            routes,
        }
    }

    pub fn load(store: &PlanningStore) -> Result<Self, StoreError> {
        let bytes = match store.read(paths::HARNESS_ROUTES) {
            Ok(bytes) => bytes,
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    schema_version: SCHEMA_VERSION,
                    routes: BTreeMap::new(),
                });
            }
            Err(error) => return Err(error),
        };
        if bytes.len() > MAX_ROUTE_FILE_BYTES {
            return Err(StoreError::MalformedState(
                "project harness routes exceed the size limit".into(),
            ));
        }
        let routes: Self = serde_json::from_slice(&bytes).map_err(|error| {
            StoreError::MalformedState(format!("project harness routes cannot be decoded: {error}"))
        })?;
        routes.validate()?;
        Ok(routes)
    }

    pub fn save(
        &self,
        store: &PlanningStore,
        expected_revision: &str,
    ) -> Result<(Vec<String>, String), StoreError> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| {
            StoreError::MalformedState(format!("project harness routes cannot be encoded: {error}"))
        })?;
        store.transaction_with_revision(
            &[(paths::HARNESS_ROUTES.to_owned(), bytes)],
            Some(expected_revision),
        )
    }

    fn validate(&self) -> Result<(), StoreError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(StoreError::MalformedState(format!(
                "unsupported project harness route schema version {}",
                self.schema_version
            )));
        }
        if self.routes.len() > TASK_ROUTABLE_CATEGORIES.len() + 1 {
            return Err(StoreError::MalformedState(
                "project harness routes contain too many categories".into(),
            ));
        }
        for (category, route) in &self.routes {
            if category != MANAGER && !TASK_ROUTABLE_CATEGORIES.contains(&category.as_str()) {
                return Err(StoreError::MalformedState(format!(
                    "unsupported project harness role '{category}'"
                )));
            }
            if !matches!(
                route.harness.as_str(),
                "pi" | "codex" | "claude" | "copilot" | "opencode" | "antigravity"
            ) {
                return Err(StoreError::MalformedState(format!(
                    "unsupported harness '{}' for project role '{category}'",
                    route.harness
                )));
            }
            if route
                .model
                .as_ref()
                .is_some_and(|model| model.trim().is_empty())
            {
                return Err(StoreError::MalformedState(format!(
                    "empty model for project role '{category}'"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "koolade-project-harness-routes-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&path)
                .status()
                .unwrap()
                .success()
        );
        path
    }

    #[test]
    fn project_routes_roundtrip_through_the_shared_planning_store() {
        let root = root();
        let store = PlanningStore::new(
            uuid::Uuid::new_v4(),
            &root,
            crate::artifacts::planning_store::StoreMode::ManagedLocal,
        );
        let routes = ProjectHarnessRoutes::with_routes(BTreeMap::from([(
            crate::persistence::harness_settings::IMPLEMENTATION.into(),
            WorkRoute {
                harness: "codex".into(),
                model: Some("gpt-5".into()),
            },
        )]));
        let revision = store.revision().unwrap();
        let (changed, _) = routes.save(&store, &revision).unwrap();
        assert_eq!(changed, vec![paths::HARNESS_ROUTES.to_owned()]);
        assert_eq!(ProjectHarnessRoutes::load(&store).unwrap(), routes);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_routes_reject_unknown_roles_and_providers() {
        let unknown_role = ProjectHarnessRoutes::with_routes(BTreeMap::from([(
            "unknown".into(),
            WorkRoute {
                harness: "codex".into(),
                model: None,
            },
        )]));
        assert!(unknown_role.validate().is_err());

        let unknown_provider = ProjectHarnessRoutes::with_routes(BTreeMap::from([(
            crate::persistence::harness_settings::IMPLEMENTATION.into(),
            WorkRoute {
                harness: "unknown".into(),
                model: None,
            },
        )]));
        assert!(unknown_provider.validate().is_err());
    }
}
