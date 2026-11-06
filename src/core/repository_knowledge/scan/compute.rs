use crate::core::repository_knowledge::facts;
use crate::core::repository_knowledge::{ArchitectureClaimV1, ClaimStatus, EvidenceRefV1};

pub(super) fn entry_claims(
    path: &str,
    content: &str,
    reference: EvidenceRefV1,
) -> Vec<ArchitectureClaimV1> {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let by_name = matches!(
        name.as_str(),
        "main.rs"
            | "lib.rs"
            | "main.py"
            | "app.py"
            | "main.go"
            | "index.ts"
            | "index.tsx"
            | "index.js"
            | "server.ts"
            | "server.js"
            | "program.cs"
            | "application.java"
    );
    let mut claims = Vec::new();
    if by_name {
        claims.push(facts::make_claim(
            format!("entry-file:{}", path_slug(path)),
            "entry points and interfaces",
            format!("{path} is a conventional application or library entry-point file."),
            reference.clone(),
            ClaimStatus::Inferred,
            70,
            Some(1),
            Some(name),
        ));
    }
    for (line_number, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let signature = [
            "fn main(",
            "func main(",
            "def main(",
            "public static void main",
            "export default",
        ]
        .iter()
        .find(|signature| trimmed.contains(**signature));
        if let Some(signature) = signature {
            let symbol = signature.trim_end_matches(['(', ' ']).to_owned();
            claims.push(facts::make_claim(
                format!("entry-symbol:{}:{}", path_slug(path), line_number + 1),
                "entry points and interfaces",
                format!(
                    "{path} declares an entry-point-like symbol on line {}.",
                    line_number + 1
                ),
                reference.clone(),
                ClaimStatus::SupportedBySource,
                85,
                Some(line_number as u32 + 1),
                Some(symbol),
            ));
            if claims.len() == 16 {
                break;
            }
        }
    }
    claims
}

pub(super) fn readme_claims(
    path: &str,
    content: &str,
    reference: EvidenceRefV1,
) -> Vec<ArchitectureClaimV1> {
    let mut in_capabilities = false;
    let mut claims = Vec::new();
    for (line_number, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
            in_capabilities = ["features", "capabilities", "what it does", "overview"]
                .iter()
                .any(|keyword| heading == *keyword);
            continue;
        }
        let Some(statement) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("1. "))
        else {
            continue;
        };
        let statement = statement.trim();
        if !in_capabilities || statement.is_empty() || statement.chars().count() > 180 {
            continue;
        }
        claims.push(facts::make_claim(
            format!("readme-capability:{}:{}", path_slug(path), line_number + 1),
            "observed capabilities",
            format!("README describes this capability: {statement}"),
            reference.clone(),
            ClaimStatus::SupportedBySource,
            75,
            Some(line_number as u32 + 1),
            None,
        ));
        if claims.len() == 16 {
            break;
        }
    }
    claims
}
pub(super) fn possible_entry_point(path: &str) -> bool {
    matches!(
        path.rsplit('/')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "main.rs"
            | "lib.rs"
            | "main.py"
            | "app.py"
            | "main.go"
            | "index.ts"
            | "index.tsx"
            | "index.js"
            | "server.ts"
            | "server.js"
            | "program.cs"
            | "application.java"
    )
}
pub(super) fn possible_execution_boundary(path: &str) -> bool {
    matches!(
        path.rsplit('/')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "dockerfile" | "docker-compose.yml" | "docker-compose.yaml" | "makefile" | "go.mod"
    )
}
pub(super) fn is_source_or_readme(path: &str) -> bool {
    super::super::source::source_extension(path).is_some()
        || path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("README.md"))
}
pub(super) fn path_slug(path: &str) -> String {
    path.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                (byte as char).to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}
