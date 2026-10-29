use base64::Engine as _;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SumEntry {
    pub(super) module: String,
    pub(super) version: String,
    pub(super) hash: String,
}

pub(super) fn parse_sums(contents: &str) -> anyhow::Result<Vec<SumEntry>> {
    anyhow::ensure!(
        contents.len() as u64 <= super::MAX_SUM_BYTES,
        "go.sum exceeds its scan limit"
    );
    let mut entries = BTreeMap::new();
    for (line_number, line) in contents.lines().enumerate() {
        anyhow::ensure!(
            line.len() <= super::MAX_LINE_BYTES,
            "go.sum line exceeds its scan limit"
        );
        if line.is_empty() {
            continue;
        }
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        anyhow::ensure!(
            fields.len() == 3,
            "go.sum line {} must contain exactly module, version, and h1 checksum",
            line_number + 1
        );
        super::validate_module_path(fields[0])?;
        let version = fields[1].strip_suffix("/go.mod").unwrap_or(fields[1]);
        super::validate_version(version)?;
        anyhow::ensure!(
            fields[2].starts_with("h1:")
                && base64::engine::general_purpose::STANDARD
                    .decode(&fields[2][3..])
                    .is_ok_and(|digest| digest.len() == 32),
            "go.sum contains an invalid h1 checksum"
        );
        let entry = SumEntry {
            module: fields[0].into(),
            version: fields[1].into(),
            hash: fields[2].into(),
        };
        anyhow::ensure!(
            entries
                .insert((entry.module.clone(), entry.version.clone()), entry)
                .is_none(),
            "go.sum contains a duplicate module/version entry"
        );
        anyhow::ensure!(
            entries.len() <= super::MAX_SUM_ENTRIES,
            "go.sum has too many entries"
        );
    }
    anyhow::ensure!(!entries.is_empty(), "go.sum has no module checksums");
    Ok(entries.into_values().collect())
}
