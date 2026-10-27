use std::path::Path;

use super::{CURRENT_SCHEMA_VERSION, FILE, HarnessSettings};

/// Absolute path of the persisted harness settings.
///
/// In test builds this is fail-closed with respect to the operator's live
/// store. Resolution layers:
/// 1. An explicit test-thread override from [`super::with_settings_path`].
/// 2. A test process that exported `KOOLADE_HOME` (the documented env-
///    isolation lever), which `state_root` already honors.
/// 3. Otherwise a uniquely named per-thread temporary directory created on
///    first use, so ambient suites can never touch the live store.
pub fn settings_path() -> std::path::PathBuf {
    #[cfg(test)]
    if let Some(path) = super::SETTINGS_PATH_OVERRIDE.with(|path| path.borrow().clone()) {
        return path;
    }
    #[cfg(test)]
    if std::env::var_os("KOOLADE_HOME")
        .filter(|value| !value.is_empty())
        .is_some()
    {
        return crate::persistence::state_root().join(FILE);
    }
    #[cfg(test)]
    {
        return super::TEST_STORE_FALLBACK.with(|fallback| {
            fallback
                .borrow_mut()
                .get_or_insert_with(|| {
                    let unique = std::env::temp_dir()
                        .join("koolade-harness-settings-test")
                        .join(uuid::Uuid::new_v4().simple().to_string());
                    let _ = std::fs::create_dir_all(&unique);
                    unique.join(FILE)
                })
                .clone()
        });
    }
    // Plain production path; the layers above make this expression
    // unreachable in test builds, hence the narrow allowance.
    #[cfg_attr(test, allow(unreachable_code))]
    {
        crate::persistence::state_root().join(FILE)
    }
}

pub fn load() -> (HarnessSettings, Option<String>) {
    load_from(&settings_path())
}

pub(super) fn load_from(path: &Path) -> (HarnessSettings, Option<String>) {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<HarnessSettings>(&bytes) {
            Ok(mut settings) if (1..=CURRENT_SCHEMA_VERSION).contains(&settings.schema_version) => {
                let original_schema_version = settings.schema_version;
                super::migration::apply(&mut settings, original_schema_version);
                if settings
                    .bootstrap_provider
                    .as_deref()
                    .is_some_and(|provider| !crate::app::dialogs::supported_provider_id(provider))
                {
                    settings.bootstrap_provider = None;
                }
                settings.schema_version = CURRENT_SCHEMA_VERSION;
                (settings, None)
            }
            Ok(settings) => (
                HarnessSettings::default(),
                Some(format!(
                    "Unsupported harness settings version {} in {}",
                    settings.schema_version,
                    path.display()
                )),
            ),
            Err(error) => (
                HarnessSettings::default(),
                Some(format!("Could not read {}: {error}", path.display())),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (HarnessSettings::default(), None)
        }
        Err(error) => (
            HarnessSettings::default(),
            Some(format!("Could not read {}: {error}", path.display())),
        ),
    }
}

pub fn save(settings: &HarnessSettings) -> anyhow::Result<()> {
    save_to(&settings_path(), settings)
}

pub(super) fn save_to(path: &Path, settings: &HarnessSettings) -> anyhow::Result<()> {
    let mut next = settings.clone();
    next.schema_version = CURRENT_SCHEMA_VERSION;
    let json = serde_json::to_string_pretty(&next)?;
    crate::artifacts::atomic_write(path, &json)
}
