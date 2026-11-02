use super::super::super::PrivateDirectory;
use std::{fs, io::Read, path::Path};

const MAX_AUTH_BYTES: u64 = 1_048_576;
const MISSING_AUTH: &str = "Codex authentication is missing. Sign in with `codex login`, set OPENAI_API_KEY or CODEX_ACCESS_TOKEN, or configure OpenAI federation credentials before using the Codex provider.";
const INVALID_AUTH: &str = "Codex saved authentication is missing, malformed, or unsafe. Re-run `codex login` or repair the auth.json file in CODEX_HOME.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AuthSource {
    Environment,
    Saved,
}

impl From<AuthSource> for crate::harness::ProviderAuthStrategy {
    fn from(source: AuthSource) -> Self {
        match source {
            AuthSource::Environment => Self::Environment,
            AuthSource::Saved => Self::Saved,
        }
    }
}

pub(super) fn prepare_auth(
    scratch: &PrivateDirectory,
    source_home: Option<&Path>,
    private_home: &Path,
    environment_auth: bool,
) -> anyhow::Result<AuthSource> {
    if environment_auth {
        return Ok(AuthSource::Environment);
    }
    let Some(source_home) = source_home else {
        anyhow::bail!("{MISSING_AUTH}");
    };
    match fs::symlink_metadata(source_home.join("auth.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("{MISSING_AUTH}")
        }
        Err(error) => anyhow::bail!("{INVALID_AUTH}: {error}"),
        Ok(_) => {}
    }
    copy_saved_auth(scratch, source_home, private_home)
        .map_err(|error| anyhow::anyhow!("{INVALID_AUTH}: {error:#}"))?;
    Ok(AuthSource::Saved)
}

pub(super) fn copy_saved_auth(
    scratch: &PrivateDirectory,
    source_home: &Path,
    private_home: &Path,
) -> anyhow::Result<()> {
    let source = source_home.join("auth.json");
    let file = open_auth_file(&source)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Codex saved authentication file is not a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_AUTH_BYTES,
        "Codex saved authentication file is not a bounded regular file"
    );
    let contents = read_bounded(file, MAX_AUTH_BYTES)?;
    anyhow::ensure!(
        serde_json::from_slice::<serde_json::Value>(&contents).is_ok_and(|value| value.is_object()),
        "Codex saved authentication file is not a valid JSON object"
    );
    let relative = private_home
        .join("auth.json")
        .strip_prefix(&scratch.path)?
        .to_string_lossy()
        .into_owned();
    scratch.write_file(&relative, &contents)?;
    Ok(())
}

fn open_auth_file(path: &Path) -> anyhow::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT prevents the final path component from
        // resolving to a reparse point before metadata is checked on the handle.
        options.custom_flags(0x0020_0000);
    }
    options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("Codex saved authentication file was not found")
        } else {
            error.into()
        }
    })
}

fn read_bounded(reader: impl Read, max_bytes: u64) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(max_bytes + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= max_bytes,
        "Codex saved authentication file exceeds its size limit"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("koolade-codex-auth-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Production boundary: imports the user's persisted Codex credential into
    /// the per-run controller HOME. A path-only unit check cannot prove the
    /// source stays intact or unrelated config/session files are excluded.
    /// Uses synthetic local files only; no CLI, Docker, network, or auth needed.
    #[test]
    fn saved_auth_is_copied_to_private_home_without_user_configuration() {
        let source = Fixture::new();
        let scratch = PrivateDirectory::create("koolade-codex-auth-test").unwrap();
        let private_home = scratch.create_directory("codex-home").unwrap();
        fs::write(
            source.0.join("auth.json"),
            r#"{"tokens":{"access":"synthetic"}}"#,
        )
        .unwrap();
        fs::write(source.0.join("config.toml"), "[hooks]\n").unwrap();
        fs::write(source.0.join("history.jsonl"), "prior session\n").unwrap();

        copy_saved_auth(&scratch, &source.0, &private_home).unwrap();

        let destination = private_home.join("auth.json");
        assert_eq!(
            fs::read_to_string(&destination).unwrap(),
            fs::read_to_string(source.0.join("auth.json")).unwrap()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(!private_home.join("config.toml").exists());
        assert!(!private_home.join("history.jsonl").exists());
        assert!(source.0.join("auth.json").exists());
    }

    /// Production boundary: rejects unsafe or malformed persisted credentials
    /// before launch. Filesystem behavior is the invariant; no external service
    /// is needed and setup is limited to a temporary directory and symlink.
    #[cfg(unix)]
    #[test]
    fn symlinked_or_malformed_saved_auth_is_rejected() {
        use std::os::unix::fs::symlink;
        let source = Fixture::new();
        let scratch = PrivateDirectory::create("koolade-codex-auth-test").unwrap();
        let private_home = scratch.create_directory("codex-home").unwrap();
        let outside = source.0.join("outside.json");
        fs::write(&outside, r#"{"token":"synthetic"}"#).unwrap();
        symlink(&outside, source.0.join("auth.json")).unwrap();
        assert!(copy_saved_auth(&scratch, &source.0, &private_home).is_err());
        fs::remove_file(source.0.join("auth.json")).unwrap();
        fs::write(source.0.join("auth.json"), "not json").unwrap();
        assert!(copy_saved_auth(&scratch, &source.0, &private_home).is_err());
        fs::write(source.0.join("auth.json"), r#"{"token":"synthetic"}"#).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(source.0.join("auth.json"))
            .unwrap()
            .set_len(MAX_AUTH_BYTES + 1)
            .unwrap();
        assert!(copy_saved_auth(&scratch, &source.0, &private_home).is_err());
    }

    #[test]
    fn environment_auth_takes_precedence_over_saved_auth() {
        let source = Fixture::new();
        fs::write(source.0.join("auth.json"), "malformed").unwrap();
        let scratch = PrivateDirectory::create("koolade-codex-auth-test").unwrap();
        let private_home = scratch.create_directory("codex-home").unwrap();

        let selected = prepare_auth(&scratch, Some(&source.0), &private_home, true).unwrap();

        assert_eq!(selected, AuthSource::Environment);
        assert_eq!(
            crate::harness::ProviderAuthStrategy::from(selected),
            crate::harness::ProviderAuthStrategy::Environment
        );
        assert!(!private_home.join("auth.json").exists());
    }

    #[test]
    fn saved_auth_is_used_when_no_environment_source_is_present() {
        let source = Fixture::new();
        fs::write(
            source.0.join("auth.json"),
            r#"{"tokens":{"access":"synthetic"}}"#,
        )
        .unwrap();
        let scratch = PrivateDirectory::create("koolade-codex-auth-test").unwrap();
        let private_home = scratch.create_directory("codex-home").unwrap();

        let selected = prepare_auth(&scratch, Some(&source.0), &private_home, false).unwrap();

        assert_eq!(selected, AuthSource::Saved);
        assert_eq!(
            crate::harness::ProviderAuthStrategy::from(selected),
            crate::harness::ProviderAuthStrategy::Saved
        );
        assert!(private_home.join("auth.json").is_file());
    }

    #[test]
    fn missing_or_malformed_auth_fails_with_actionable_guidance() {
        let scratch = PrivateDirectory::create("koolade-codex-auth-test").unwrap();
        let private_home = scratch.create_directory("codex-home").unwrap();
        let missing = Fixture::new();
        let error = prepare_auth(&scratch, Some(&missing.0), &private_home, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("codex login"));

        fs::write(missing.0.join("auth.json"), "malformed").unwrap();
        let error = prepare_auth(&scratch, Some(&missing.0), &private_home, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("malformed"));
        assert!(error.contains("CODEX_HOME"));
    }

    #[test]
    fn bounded_auth_reader_rejects_growth_past_limit() {
        let bytes = std::io::Cursor::new(vec![0; MAX_AUTH_BYTES as usize + 1]);
        assert!(read_bounded(bytes, MAX_AUTH_BYTES).is_err());
    }
}
