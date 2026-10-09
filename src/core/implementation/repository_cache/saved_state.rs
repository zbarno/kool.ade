use super::*;

impl RepositoryCache {
    pub(in crate::core::implementation) fn from_saved_state(
        state: &super::super::Implementation,
        runner: &Runner,
    ) -> anyhow::Result<Self> {
        let identity = state
            .repository_identity
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Saved repository identity is missing"))?;
        let path = state
            .repository_cache
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Saved repository cache path is missing"))?;
        anyhow::ensure!(
            path == cache_path(&identity)?,
            "Saved repository cache is outside Kool.ad/e's app-owned cache"
        );
        anyhow::ensure!(
            path.file_name()
                .is_some_and(|name| { name == std::ffi::OsStr::new(&format!("{identity}.git")) }),
            "Saved repository cache path does not match its identity"
        );
        verify_bare_cache(&path, runner)?;
        let origin_url = configured_value(&path, "remote.origin.url", runner)?;
        let push_url = configured_value(&path, "remote.origin.pushurl", runner)?
            .or_else(|| origin_url.clone());
        let fetch_url =
            configured_value(&path, "koolade.fetchUrl", runner)?.or_else(|| origin_url.clone());
        let push_identity_url = state.push_repository.clone().or_else(|| {
            if push_url == fetch_url {
                origin_url.clone()
            } else {
                push_url.clone()
            }
        });
        if let Some(origin) = origin_url.as_deref() {
            anyhow::ensure!(
                identity_for_repository(
                    origin,
                    fetch_url.as_deref(),
                    push_url.as_deref(),
                    Path::new("."),
                )? == identity,
                "Saved repository cache endpoints do not match its identity"
            );
        }
        Ok(Self {
            identity,
            path,
            origin_url,
            push_url,
            push_identity_url,
            fetch_url,
        })
    }
}
