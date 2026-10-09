use super::super::*;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub(in crate::core::implementation) struct ReportCheckClone {
    path: PathBuf,
    patch: PathBuf,
}

impl ReportCheckClone {
    pub(in crate::core::implementation) fn create(
        state: &Implementation,
        artifact_dir: &Path,
        runner: &Runner,
    ) -> anyhow::Result<Self> {
        crate::core::implementation::initial_reconciliation::support::generated::quarantine_untrusted_ignored(
            runner,
            state,
            artifact_dir,
        )?;
        let source = &state.task_repository;
        let key = task_repository::allocation_key(state);
        let nonce = chrono::Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_default()
            .unsigned_abs()
            & 0x0000_ffff_ffff_ffff;
        let path = source.with_file_name(format!("{key}-verification-{nonce:012x}"));
        let patch = path.with_extension("patch");
        anyhow::ensure!(
            !path.exists() && !patch.exists(),
            "Temporary report verification clone already exists"
        );

        let result = Self::populate(source, &path, &patch, state, artifact_dir, runner);
        if result.is_err() {
            let _ = fs::remove_dir_all(&path);
            let _ = fs::remove_file(&patch);
        }
        result?;
        Ok(Self { path, patch })
    }

    pub(in crate::core::implementation) fn path(&self) -> &Path {
        &self.path
    }

    fn populate(
        source: &Path,
        destination: &Path,
        patch_path: &Path,
        state: &Implementation,
        artifact_dir: &Path,
        runner: &Runner,
    ) -> anyhow::Result<()> {
        let source_text = source
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 task repository path"))?;
        let destination_text = destination
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 verification clone path"))?;
        runner.git(
            source,
            &[
                "-c",
                "user.name=Kool.ad/e",
                "-c",
                "user.email=koolade@localhost",
                "clone",
                "--quiet",
                "--local",
                "--no-hardlinks",
                "--branch",
                &state.branch,
                source_text,
                destination_text,
            ],
        )?;

        if let Ok(origin) = runner.git(source, &["config", "--get", "remote.origin.url"]) {
            runner.git(destination, &["remote", "set-url", "origin", &origin])?;
        }
        if let Ok(push_url) = runner.git(source, &["config", "--get", "remote.origin.pushurl"]) {
            runner.git(destination, &["config", "remote.origin.pushurl", &push_url])?;
        }

        runner.git_to_file(source, &["diff", "--binary", "HEAD"], patch_path)?;
        if fs::metadata(patch_path)?.len() > 0 {
            let patch_text = patch_path
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Non-UTF8 verification patch path"))?;
            let apply = runner.git(destination, &["apply", "--binary", patch_text]);
            let _ = fs::remove_file(patch_path);
            apply?;
        }

        let visible = runner.git_nul_records(
            source,
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )?;
        for relative in visible {
            copy_entry(source, destination, &relative)?;
        }

        let runtime_config = crate::harness::pi_sandbox::runtime_config::paths_with_source(
            source,
            runner.runtime_config_source.as_deref(),
        )?;
        for relative in runtime_config {
            let relative = safe_relative(&relative)?;
            let placeholder = destination.join(&relative);
            if !placeholder.exists() {
                if let Some(parent) = placeholder.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(placeholder, [])?;
            }
        }

        let runtime_config = crate::harness::pi_sandbox::runtime_config::paths_with_source(
            source,
            runner.runtime_config_source.as_deref(),
        )?;
        for relative in
            crate::core::implementation::initial_reconciliation::support::generated::trusted(
                runner,
                state,
                artifact_dir,
            )?
            .difference(&runtime_config)
        {
            copy_entry(source, destination, relative)?;
        }
        Ok(())
    }
}

impl Drop for ReportCheckClone {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
        let _ = fs::remove_file(&self.patch);
    }
}

fn safe_relative(relative: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(relative);
    anyhow::ensure!(
        !path.is_absolute()
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "Verification clone input path is not a safe relative path"
    );
    Ok(path.to_path_buf())
}

fn copy_entry(source: &Path, destination: &Path, relative: &str) -> anyhow::Result<()> {
    let relative = safe_relative(relative)?;
    let source_root = source.canonicalize()?;
    let source_path = source.join(&relative);
    let target_path = destination.join(&relative);
    let metadata = fs::symlink_metadata(&source_path)?;
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if metadata.file_type().is_symlink() {
        let link = fs::read_link(&source_path)?;
        let resolved = source_path.parent().unwrap().join(&link).canonicalize()?;
        anyhow::ensure!(
            resolved.starts_with(&source_root),
            "Verification clone input symlink escapes the task repository"
        );
        if target_path.exists() || fs::symlink_metadata(&target_path).is_ok() {
            fs::remove_file(&target_path)?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(link, target_path)?;
        #[cfg(not(unix))]
        anyhow::bail!("Verification clone symlinks are unsupported on this platform");
    } else {
        anyhow::ensure!(
            metadata.is_file() && source_path.canonicalize()?.starts_with(&source_root),
            "Verification clone input is not a regular in-repository file"
        );
        fs::copy(source_path, target_path)?;
    }
    Ok(())
}
