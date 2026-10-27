use std::collections::BTreeMap;

use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

mod policy;

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SourceInspection {
    pub(crate) has_docker_uri: bool,
    pub(crate) has_invalid_docker_entry: bool,
    pub(crate) has_unapproved_source: bool,
    pub(crate) has_active_source: bool,
    pub(crate) exact_entries: usize,
}

pub(super) fn inspect_source_contents(
    contents: &[u8],
    deb822: bool,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<SourceInspection> {
    let text = std::str::from_utf8(contents)?;
    if deb822 {
        inspect_deb822_sources(text, recipe)
    } else {
        inspect_list_sources(text, recipe)
    }
}

fn inspect_list_sources(
    contents: &str,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<SourceInspection> {
    let mut inspection = SourceInspection::default();
    for line in contents.lines().map(str::trim) {
        let line = line
            .split_once('#')
            .map_or(line, |(source, _)| source)
            .trim();
        if line.is_empty() {
            continue;
        }
        merge_entry(&mut inspection, policy::inspect_list_entry(line, recipe)?);
    }
    Ok(inspection)
}

fn inspect_deb822_sources(
    contents: &str,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<SourceInspection> {
    let mut inspection = SourceInspection::default();
    let mut fields = BTreeMap::<String, String>::new();
    let mut previous_field = None::<String>;
    for line in contents.lines().chain(std::iter::once("")) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !fields.is_empty() {
                merge_entry(
                    &mut inspection,
                    policy::inspect_deb822_entry(&fields, recipe)?,
                );
            }
            fields.clear();
            previous_field = None;
            continue;
        }
        if line.trim_start().starts_with('#') {
            continue;
        }
        if line.as_bytes().first().is_some_and(u8::is_ascii_whitespace) {
            let Some(key) = previous_field.as_ref() else {
                anyhow::bail!("malformed Debian source continuation line");
            };
            let value = fields
                .get_mut(key)
                .ok_or_else(|| anyhow::anyhow!("Debian source continuation has no field"))?;
            value.push(' ');
            value.push_str(trimmed);
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("malformed Debian source field"))?;
        let key = key.trim().to_ascii_lowercase();
        anyhow::ensure!(
            !key.is_empty()
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
            "malformed Debian source field name"
        );
        anyhow::ensure!(
            fields
                .insert(key.clone(), value.trim().to_owned())
                .is_none(),
            "duplicate Debian source field"
        );
        previous_field = Some(key);
    }
    Ok(inspection)
}

fn merge_entry(inspection: &mut SourceInspection, entry: policy::EntryInspection) {
    inspection.has_docker_uri |= entry.has_docker_uri;
    inspection.has_active_source |= entry.active;
    inspection.has_unapproved_source |= entry.active && !entry.approved;
    inspection.has_invalid_docker_entry |= entry.active && entry.has_docker_uri && !entry.approved;
    inspection.exact_entries += usize::from(entry.exact_docker);
}
