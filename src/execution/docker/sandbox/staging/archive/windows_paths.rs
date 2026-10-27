use std::{cell::Cell, cmp::Ordering, collections::BTreeMap, path::Path, rc::Rc};

pub(super) fn validate_path(path: &Path) -> anyhow::Result<()> {
    let value = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Workspace path is not valid Unicode"))?;
    anyhow::ensure!(
        !value.starts_with('/') && !value.starts_with('\\'),
        "Windows workspace path is absolute"
    );
    for component in value.split(['/', '\\']) {
        anyhow::ensure!(
            !component.is_empty() && component != "." && component != "..",
            "Windows workspace path has an empty or traversal component"
        );
        anyhow::ensure!(
            component.encode_utf16().count() <= 255,
            "Windows workspace path component exceeds the filesystem limit"
        );
        anyhow::ensure!(
            !component.chars().any(|character| {
                character <= '\u{1f}'
                    || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
            }),
            "Windows workspace path contains a reserved character"
        );
        anyhow::ensure!(
            !matches!(component.chars().last(), Some(' ' | '.')),
            "Windows workspace path ends in a normalized-away character"
        );
        anyhow::ensure!(
            !is_short_name_alias_shaped(component),
            "Windows workspace path resembles an 8.3 short-name alias"
        );
        anyhow::ensure!(
            !is_reserved_device_name(component),
            "Windows workspace path uses a reserved device name"
        );
    }
    Ok(())
}

fn is_short_name_alias_shaped(component: &str) -> bool {
    if !component.is_ascii() {
        return false;
    }
    let (base, extension) = component.split_once('.').unwrap_or((component, ""));
    if base.len() > 8 || extension.len() > 3 {
        return false;
    }
    let Some((prefix, sequence)) = base.rsplit_once('~') else {
        return false;
    };
    prefix.len() <= 6 && !sequence.is_empty() && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn validate_collisions<'a>(paths: impl Iterator<Item = &'a Path>) -> anyhow::Result<()> {
    let paths = paths.collect::<Vec<_>>();
    for path in &paths {
        validate_path(path)?;
    }
    validate_case_collisions_with_separators(paths.into_iter(), &['/', '\\'])
}

pub(super) fn validate_case_collisions<'a>(
    paths: impl Iterator<Item = &'a Path>,
) -> anyhow::Result<()> {
    validate_case_collisions_with_separators(paths, &['/'])
}

fn validate_case_collisions_with_separators<'a>(
    paths: impl Iterator<Item = &'a Path>,
    separators: &[char],
) -> anyhow::Result<()> {
    let comparison_failed = Rc::new(Cell::new(false));
    // Ord sets this shared flag if Windows' comparison API fails; preserve BTreeMap's efficient
    // lookup and reject the archive after all comparisons rather than changing the sort order.
    #[allow(clippy::mutable_key_type)]
    let mut seen = BTreeMap::<CaseInsensitivePath, String>::new();
    #[allow(clippy::mutable_key_type)]
    let mut canonical_paths = BTreeMap::<CaseInsensitivePath, String>::new();
    for path in paths {
        let raw = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Windows workspace path is not valid Unicode"))?;
        let mut original_prefix = String::new();
        for name in raw.split(|character| separators.contains(&character)) {
            if !original_prefix.is_empty() {
                original_prefix.push('/');
            }
            original_prefix.push_str(name);

            let key = case_insensitive_path(&original_prefix, &comparison_failed);
            if let Some(existing) = seen.get(&key) {
                anyhow::ensure!(
                    existing == &original_prefix,
                    "Workspace paths collide after case normalization"
                );
            } else {
                seen.insert(key, original_prefix.clone());
            }
        }
        let key = case_insensitive_path(&original_prefix, &comparison_failed);
        if let Some(existing) = canonical_paths.get(&key) {
            anyhow::ensure!(
                existing == raw,
                "Workspace paths have multiple spellings for the same location"
            );
        } else {
            canonical_paths.insert(key, raw.to_owned());
        }
    }
    anyhow::ensure!(
        !comparison_failed.get(),
        "Workspace paths could not be compared safely"
    );
    Ok(())
}

#[cfg(not(windows))]
#[derive(Clone)]
struct CaseInsensitivePath {
    normalized: String,
}

#[cfg(not(windows))]
impl PartialEq for CaseInsensitivePath {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

#[cfg(not(windows))]
impl Eq for CaseInsensitivePath {}

#[cfg(not(windows))]
impl PartialOrd for CaseInsensitivePath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(not(windows))]
impl Ord for CaseInsensitivePath {
    fn cmp(&self, other: &Self) -> Ordering {
        focaccia::CaseFold::Full.casecmp(&self.normalized, &other.normalized)
    }
}

#[cfg(not(windows))]
fn case_insensitive_path(value: &str, _comparison_failed: &Rc<Cell<bool>>) -> CaseInsensitivePath {
    use unicode_normalization::UnicodeNormalization;

    CaseInsensitivePath {
        normalized: value.nfd().collect(),
    }
}

#[cfg(windows)]
#[derive(Clone)]
struct CaseInsensitivePath {
    utf16: Vec<u16>,
    comparison_failed: Rc<Cell<bool>>,
}

#[cfg(windows)]
impl PartialEq for CaseInsensitivePath {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

#[cfg(windows)]
impl Eq for CaseInsensitivePath {}

#[cfg(windows)]
impl PartialOrd for CaseInsensitivePath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(windows)]
impl Ord for CaseInsensitivePath {
    fn cmp(&self, other: &Self) -> Ordering {
        match windows_ordinal_compare(&self.utf16, &other.utf16) {
            Some(ordering) => ordering,
            None => {
                self.comparison_failed.set(true);
                other.comparison_failed.set(true);
                self.utf16.cmp(&other.utf16)
            }
        }
    }
}

#[cfg(windows)]
fn case_insensitive_path(value: &str, comparison_failed: &Rc<Cell<bool>>) -> CaseInsensitivePath {
    CaseInsensitivePath {
        utf16: value.encode_utf16().collect(),
        comparison_failed: Rc::clone(comparison_failed),
    }
}

#[cfg(windows)]
fn windows_ordinal_compare(left: &[u16], right: &[u16]) -> Option<Ordering> {
    let left_len = i32::try_from(left.len()).ok()?;
    let right_len = i32::try_from(right.len()).ok()?;
    let result =
        unsafe { compare_string_ordinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    match result {
        1 => Some(Ordering::Less),
        2 => Some(Ordering::Equal),
        3 => Some(Ordering::Greater),
        _ => None,
    }
}

#[cfg(all(test, windows))]
pub(super) fn windows_ordinal_eq_ignore_case(left: &str, right: &str) -> anyhow::Result<bool> {
    #[cfg(windows)]
    {
        let left = left.encode_utf16().collect::<Vec<_>>();
        let right = right.encode_utf16().collect::<Vec<_>>();
        return windows_ordinal_compare(&left, &right)
            .map(|ordering| ordering == Ordering::Equal)
            .ok_or_else(|| anyhow::anyhow!("Windows paths could not be compared safely"));
    }
    #[cfg(not(windows))]
    Ok(left
        .chars()
        .flat_map(char::to_uppercase)
        .eq(right.chars().flat_map(char::to_uppercase)))
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "CompareStringOrdinal"]
    fn compare_string_ordinal(
        lpstring1: *const u16,
        cchcount1: i32,
        lpstring2: *const u16,
        cchcount2: i32,
        bignorecase: i32,
    ) -> i32;
}

fn is_reserved_device_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].into_iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(suffix, "¹" | "²" | "³")
                || (suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit() && suffix != "0")
        })
    })
}

#[cfg(test)]
#[path = "windows_paths/tests.rs"]
mod tests;
