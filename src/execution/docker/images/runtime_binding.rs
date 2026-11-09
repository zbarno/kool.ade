//! Typed binding of a runtime plan to the project facts that authorized it.
//!
//! The content-addressed *recipe digest* stays deliberately independent of
//! any single repository so verified composite images can be shared across
//! projects (see `images/project_plan.rs`). This type adds the audit-grade
//! association — workspace project, repository, source revision, selected
//! target, and the exact approved recipe set — without weakening that
//! content addressing: sessions carry the binding digest as provenance so
//! operators can correlate a connected runtime with the project facts that
//! justified it, while the image cache itself remains reusable.

use sha2::{Digest, Sha256};

/// Identity of the project facts a runtime plan was derived from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePlanBinding {
    pub project_id: String,
    pub repository_id: String,
    pub source_sha: String,
    pub selected_target_id: Option<String>,
    /// Canonical (sorted, de-duplicated) approved recipe identifiers.
    pub recipe_ids: Vec<String>,
}

impl RuntimePlanBinding {
    pub fn new(
        project_id: impl Into<String>,
        repository_id: impl Into<String>,
        source_sha: impl Into<String>,
        selected_target_id: Option<String>,
        recipe_ids: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut recipe_ids: Vec<String> = recipe_ids
            .into_iter()
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty())
            .collect();
        recipe_ids.sort();
        recipe_ids.dedup();
        Self {
            project_id: project_id.into(),
            repository_id: repository_id.into(),
            source_sha: source_sha.into(),
            selected_target_id,
            recipe_ids,
        }
    }

    /// Stable SHA-256 identity over the binding fields, rendered with an
    /// explicit algorithm prefix like other runtime digests. Equal fact sets
    /// digest identically regardless of input ordering; any differing
    /// project, repository, revision, target, or recipe set digests
    /// differently.
    pub fn binding_digest(&self) -> String {
        let canonical = format!(
            "koolade-runtime-binding-v1\n{}\n{}\n{}\n{}\n{}",
            self.project_id,
            self.repository_id,
            self.source_sha,
            self.selected_target_id.as_deref().unwrap_or("-"),
            self.recipe_ids.join(","),
        );
        let hashed = Sha256::digest(canonical.as_bytes());
        format!("sha256:{hashed:x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(
        project: &str,
        repository: &str,
        sha: char,
        target: Option<&str>,
        recipes: &[&str],
    ) -> RuntimePlanBinding {
        RuntimePlanBinding::new(
            project,
            repository,
            std::iter::repeat_n(sha, 40).collect::<String>(),
            target.map(ToOwned::to_owned),
            recipes.iter().map(|id| (*id).to_owned()),
        )
    }

    #[test]
    fn binding_digest_is_stable_under_input_ordering() {
        let first = binding(
            "project",
            "repository",
            'a',
            Some("target"),
            &["beta", "alpha", "alpha"],
        );
        let reordered = binding(
            "project",
            "repository",
            'a',
            Some("target"),
            &["alpha", "alpha", "beta"],
        );
        assert_eq!(first.binding_digest(), reordered.binding_digest());
    }

    #[test]
    fn binding_digest_changes_with_any_bound_fact() {
        let base = binding("project", "repository", 'a', Some("target"), &["recipe"]);
        let digest = base.binding_digest();
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), "sha256:".len() + 64);
        assert_ne!(
            base.binding_digest(),
            binding(
                "other-project",
                "repository",
                'a',
                Some("target"),
                &["recipe"]
            )
            .binding_digest()
        );
        assert_ne!(
            base.binding_digest(),
            binding(
                "project",
                "other-repository",
                'a',
                Some("target"),
                &["recipe"]
            )
            .binding_digest()
        );
        assert_ne!(
            base.binding_digest(),
            binding("project", "repository", 'b', Some("target"), &["recipe"]).binding_digest()
        );
        assert_ne!(
            base.binding_digest(),
            binding(
                "project",
                "repository",
                'a',
                Some("other-target"),
                &["recipe"]
            )
            .binding_digest()
        );
        assert_ne!(
            base.binding_digest(),
            binding(
                "project",
                "repository",
                'a',
                Some("target"),
                &["other-recipe"]
            )
            .binding_digest()
        );
        assert_ne!(
            base.binding_digest(),
            binding("project", "repository", 'a', None, &["recipe"]).binding_digest()
        );
    }
}
