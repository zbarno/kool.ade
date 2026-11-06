use std::collections::{BTreeMap, BTreeSet};

use super::{
    ArchitectureClaimV1, ClaimEvidenceV1, ClaimStatus, EvidenceRefV1,
    source::{TreeFile, is_manifest, is_test, source_extension},
};

pub(super) fn languages(files: &BTreeMap<String, TreeFile>) -> Vec<String> {
    files
        .keys()
        .filter_map(|path| source_extension(path))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub(super) fn manifest_paths(files: &BTreeMap<String, TreeFile>) -> Vec<String> {
    files
        .keys()
        .filter(|path| is_manifest(path))
        .take(128)
        .cloned()
        .collect()
}

pub(super) fn test_paths(files: &BTreeMap<String, TreeFile>) -> Vec<String> {
    files
        .keys()
        .filter(|path| is_test(path))
        .take(128)
        .cloned()
        .collect()
}

pub(super) fn subsystem_paths(
    directories: &BTreeMap<String, Vec<String>>,
) -> Vec<(String, String)> {
    directories
        .iter()
        .filter_map(|(directory, files)| {
            files.first().map(|file| (directory.clone(), file.clone()))
        })
        .take(64)
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn make_claim(
    id: impl Into<String>,
    area: impl Into<String>,
    statement: impl Into<String>,
    mut evidence: EvidenceRefV1,
    status: ClaimStatus,
    confidence: u8,
    line: Option<u32>,
    symbol: Option<String>,
) -> ArchitectureClaimV1 {
    evidence.line_start = line;
    evidence.line_end = line;
    evidence.symbol = symbol;
    ArchitectureClaimV1 {
        id: id.into(),
        area: area.into(),
        statement: statement.into(),
        evidence: ClaimEvidenceV1 {
            status,
            confidence,
            references: vec![evidence],
        },
    }
}

pub(super) fn dependency_claims(
    path: &str,
    text: &str,
    evidence: EvidenceRefV1,
) -> Vec<ArchitectureClaimV1> {
    let mut claims = Vec::new();
    let mut in_dependencies = false;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if path.ends_with("Cargo.toml") {
            if trimmed.starts_with('[') {
                in_dependencies =
                    trimmed.contains("dependencies") || trimmed.contains("workspace.dependencies");
                continue;
            }
            if in_dependencies && let Some((name, _)) = trimmed.split_once('=') {
                let name = name.trim();
                if !valid_dependency_name(name) {
                    continue;
                }
                claims.push(make_claim(
                    format!("dependency:{}", slug(name)),
                    "dependencies",
                    format!("Cargo manifest declares dependency {name}."),
                    evidence.clone(),
                    ClaimStatus::SupportedBySource,
                    90,
                    Some(index as u32 + 1),
                    Some(name.to_owned()),
                ));
            }
        } else if path.ends_with("package.json") && trimmed.contains('"') {
            let section = trimmed
                .trim_start_matches('"')
                .split('"')
                .next()
                .unwrap_or_default();
            if matches!(
                section,
                "dependencies" | "devDependencies" | "peerDependencies"
            ) {
                in_dependencies = true;
                continue;
            }
            if trimmed.starts_with('}') || trimmed.starts_with(']') {
                in_dependencies = false;
            }
            if in_dependencies
                && let Some(name) = trimmed
                    .strip_prefix('"')
                    .and_then(|value| value.split('"').next())
                && valid_dependency_name(name)
            {
                claims.push(make_claim(
                    format!("dependency:{}", slug(name)),
                    "dependencies",
                    format!("Package manifest declares dependency {name}."),
                    evidence.clone(),
                    ClaimStatus::SupportedBySource,
                    90,
                    Some(index as u32 + 1),
                    Some(name.to_owned()),
                ));
            }
        } else if path.ends_with("go.mod") {
            let name = trimmed
                .strip_prefix("require ")
                .or_else(|| trimmed.strip_prefix("\t"))
                .and_then(|value| value.split_ascii_whitespace().next())
                .filter(|value| value.contains('.'));
            if let Some(name) = name {
                claims.push(make_claim(
                    format!("dependency:{}", slug(name)),
                    "dependencies",
                    format!("Go module declares dependency {name}."),
                    evidence.clone(),
                    ClaimStatus::SupportedBySource,
                    90,
                    Some(index as u32 + 1),
                    Some(name.to_owned()),
                ));
            }
        }
        if claims.len() >= 128 {
            break;
        }
    }
    claims
}

pub(super) fn boundary_claim(path: &str, evidence: EvidenceRefV1) -> Option<ArchitectureClaimV1> {
    let (statement, symbol) = match path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase()
        .as_str()
    {
        "dockerfile" => (
            "A Dockerfile declares a container build or runtime boundary.",
            "Dockerfile",
        ),
        "docker-compose.yml" | "docker-compose.yaml" => (
            "A Compose file declares one or more service execution boundaries.",
            "services",
        ),
        "makefile" => (
            "A Makefile declares named build or test entry points.",
            "Makefile",
        ),
        "go.mod" => (
            "A Go module declares a language-level build boundary.",
            "module",
        ),
        _ => return None,
    };
    Some(make_claim(
        format!("boundary:{}", slug(path)),
        "execution boundary",
        statement,
        evidence,
        ClaimStatus::SupportedBySource,
        85,
        Some(1),
        Some(symbol.into()),
    ))
}

fn valid_dependency_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"@_./-".contains(&byte))
}

fn slug(value: &str) -> String {
    let value = value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                byte as char
            } else {
                '-'
            }
        })
        .collect::<String>();
    value.trim_matches('-').to_ascii_lowercase()
}
