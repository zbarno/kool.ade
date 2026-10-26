use crate::app::bootstrap_remediation::TrustedArgument;

// Docker's rootless prerequisites document 231072:65536 as a sufficient
// subordinate ID mapping; the inclusive range is 231072-296607.
const RANGE_START: u32 = 231_072;
const RANGE_COUNT: u32 = 65_536;
const RANGE_END: u32 = RANGE_START + RANGE_COUNT - 1;
const SUBUID_FILE: &str = "/etc/subuid";
const SUBGID_FILE: &str = "/etc/subgid";
const MAX_MAPPING_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CurrentAccount {
    pub(super) uid: u32,
    pub(super) name: String,
}

pub(super) fn verify_free_range_for_current_user() -> anyhow::Result<CurrentAccount> {
    #[cfg(target_os = "linux")]
    {
        let account = current_account()?;
        anyhow::ensure!(
            account.uid != 0,
            "rootless subordinate IDs cannot target root"
        );
        let subuid = read_root_owned_file(SUBUID_FILE)?;
        let subgid = read_root_owned_file(SUBGID_FILE)?;
        validate_mapping_file(&subuid, &account, "subuid")?;
        validate_mapping_file(&subgid, &account, "subgid")?;
        Ok(account)
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("subordinate ID operations are Linux-only")
    }
}

pub(crate) fn validate_current_mappings_for_uid(
    expected_uid: u32,
    expected_name: &str,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let account = current_account()?;
        anyhow::ensure!(
            account.uid == expected_uid && account.name == expected_name,
            "current remediation account identity changed before usermod"
        );
        let subuid = read_root_owned_file(SUBUID_FILE)?;
        let subgid = read_root_owned_file(SUBGID_FILE)?;
        validate_mapping_file(&subuid, &account, "subuid")?;
        validate_mapping_file(&subgid, &account, "subgid")?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (expected_uid, expected_name);
        anyhow::bail!("subordinate ID operations are Linux-only")
    }
}

pub(super) fn arguments_for(account: &CurrentAccount) -> Vec<TrustedArgument> {
    vec![
        TrustedArgument::UsermodAddSubuids,
        TrustedArgument::UsermodSubuidRange {
            start: RANGE_START,
            end: RANGE_END,
        },
        TrustedArgument::UsermodAddSubgids,
        TrustedArgument::UsermodSubgidRange {
            start: RANGE_START,
            end: RANGE_END,
        },
        TrustedArgument::UsermodEndOfOptions,
        TrustedArgument::CurrentUserName {
            uid: account.uid,
            name: account.name.clone(),
        },
    ]
}

#[cfg(target_os = "linux")]
fn current_account() -> anyhow::Result<CurrentAccount> {
    use std::{ffi::CStr, ptr};

    let uid = unsafe { libc::geteuid() };
    anyhow::ensure!(uid != 0, "rootless subordinate IDs cannot target root");
    let mut record = std::mem::MaybeUninit::<libc::passwd>::zeroed();
    let mut buffer = vec![0_i8; 16 * 1024];
    let mut result = ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    anyhow::ensure!(
        status == 0 && !result.is_null(),
        "current Linux account could not be resolved"
    );
    let record = unsafe { record.assume_init() };
    anyhow::ensure!(
        !record.pw_name.is_null(),
        "current Linux account has no login name"
    );
    let name = unsafe { CStr::from_ptr(record.pw_name) }
        .to_str()?
        .to_owned();
    anyhow::ensure!(
        valid_login_name(&name),
        "current Linux account has an unsupported login name"
    );
    Ok(CurrentAccount { uid, name })
}

#[cfg(not(target_os = "linux"))]
fn current_account() -> anyhow::Result<CurrentAccount> {
    anyhow::bail!("subordinate ID operations are Linux-only")
}

fn valid_login_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'$'))
}

#[cfg(target_os = "linux")]
fn read_root_owned_file(path: &str) -> anyhow::Result<String> {
    use std::{
        fs::OpenOptions,
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };

    let etc = std::fs::symlink_metadata("/etc")?;
    anyhow::ensure!(
        etc.is_dir() && !etc.file_type().is_symlink() && etc.uid() == 0 && etc.mode() & 0o022 == 0,
        "system account directory is not trusted"
    );
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == 0
            && metadata.mode() & 0o022 == 0
            && metadata.len() <= MAX_MAPPING_FILE_BYTES,
        "{path} is not a trusted bounded root-owned file"
    );
    let mut contents = String::new();
    file.take(MAX_MAPPING_FILE_BYTES + 1)
        .read_to_string(&mut contents)?;
    anyhow::ensure!(
        contents.len() as u64 <= MAX_MAPPING_FILE_BYTES,
        "{path} exceeds the subordinate ID file size limit"
    );
    Ok(contents)
}

#[cfg(not(target_os = "linux"))]
fn read_root_owned_file(_path: &str) -> anyhow::Result<String> {
    anyhow::bail!("subordinate ID operations are Linux-only")
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Mapping {
    start: u64,
    end_exclusive: u64,
}

fn validate_mapping_file(
    contents: &str,
    account: &CurrentAccount,
    kind: &str,
) -> anyhow::Result<()> {
    let mut mappings = Vec::<Mapping>::new();
    let candidate_start = u64::from(RANGE_START);
    let candidate_end = u64::from(RANGE_END) + 1;
    let numeric_uid = account.uid.to_string();
    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split(':').collect::<Vec<_>>();
        anyhow::ensure!(
            fields.len() == 3
                && !fields[0].is_empty()
                && fields[0].bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'$')
                }),
            "{kind} contains a partial or malformed subordinate ID entry"
        );
        let start = fields[1]
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("{kind} contains a malformed subordinate ID start"))?;
        let count = fields[2]
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("{kind} contains a malformed subordinate ID count"))?;
        let end_exclusive = start
            .checked_add(count)
            .ok_or_else(|| anyhow::anyhow!("{kind} subordinate ID range overflows"))?;
        anyhow::ensure!(
            start > 0 && count > 0 && end_exclusive <= u64::from(u32::MAX) + 1,
            "{kind} contains an out-of-range subordinate ID entry"
        );
        let owner = fields[0].to_owned();
        anyhow::ensure!(
            owner != account.name && owner != numeric_uid,
            "current user already has a {kind} subordinate ID mapping"
        );
        anyhow::ensure!(
            end_exclusive <= candidate_start || start >= candidate_end,
            "fixed rootless subordinate ID range overlaps an existing {kind} mapping"
        );
        let mapping = Mapping {
            start,
            end_exclusive,
        };
        anyhow::ensure!(
            mappings.iter().all(|existing| {
                existing.end_exclusive <= mapping.start || mapping.end_exclusive <= existing.start
            }),
            "{kind} contains overlapping subordinate ID mappings"
        );
        mappings.push(mapping);
    }
    Ok(())
}

#[cfg(test)]
#[path = "subids/tests.rs"]
mod tests;
