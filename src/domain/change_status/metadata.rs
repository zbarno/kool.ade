use serde::{Deserialize, Serialize};

use super::super::ArtifactIdentity;
use super::ChangeStatus;

const MARKER: &str = "<!-- packet-change:v1 ";
const STATUS_PREFIX: &str = "**Status:**";

mod comparison;
mod rewrite;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeMetadata {
    pub schema_version: u32,
    pub uid: String,
    pub display_id: String,
    pub status: ChangeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_comparison: Option<crate::domain::PlanComparison>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comparison_history: Vec<crate::domain::PlanComparison>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_alt: Option<String>,
}

impl ChangeMetadata {
    pub fn new(identity: &ArtifactIdentity, status: ChangeStatus) -> Self {
        Self {
            schema_version: 1,
            uid: identity.uid.clone(),
            display_id: identity.display_id.clone(),
            status,
            plan_comparison: None,
            comparison_history: Vec::new(),
            selected_alt: None,
        }
    }

    pub fn from_markdown(markdown: &str) -> anyhow::Result<Option<Self>> {
        let mut found = None;
        for line in markdown.lines().filter(|line| line.starts_with(MARKER)) {
            anyhow::ensure!(
                found.is_none(),
                "Markdown contains duplicate change metadata"
            );
            let json = line
                .strip_prefix(MARKER)
                .and_then(|line| line.strip_suffix(" -->"))
                .ok_or_else(|| anyhow::anyhow!("Malformed Packet change metadata marker"))?;
            let metadata: Self = serde_json::from_str(json)?;
            anyhow::ensure!(
                matches!(metadata.schema_version, 1 | 2),
                "Unsupported change metadata schema"
            );
            anyhow::ensure!(
                uuid::Uuid::parse_str(&metadata.uid).is_ok()
                    && !metadata.display_id.trim().is_empty(),
                "Malformed Packet change metadata fields"
            );
            anyhow::ensure!(
                metadata
                    .selected_alt
                    .as_deref()
                    .is_none_or(|id| ["A", "B"].contains(&id)),
                "Malformed selected alternative"
            );
            anyhow::ensure!(
                metadata.selected_alt.is_none() || metadata.plan_comparison.is_some(),
                "Selected alternative has no comparison"
            );
            anyhow::ensure!(
                metadata.schema_version == 2
                    || (metadata.plan_comparison.is_none()
                        && metadata.comparison_history.is_empty()
                        && metadata.selected_alt.is_none()),
                "Compare Plans metadata requires schema version 2"
            );
            if let Some(comparison) = &metadata.plan_comparison {
                anyhow::ensure!(
                    comparison.alternatives.len() == 2
                        && comparison.alternatives[0].id == "A"
                        && comparison.alternatives[1].id == "B"
                        && ["A", "B"].contains(&comparison.recommendation.plan_id.as_str())
                        && comparison
                            .selected_plan
                            .as_deref()
                            .is_none_or(|id| ["A", "B"].contains(&id))
                        && comparison.selected_plan == metadata.selected_alt,
                    "Malformed plan comparison metadata"
                );
            }
            for comparison in &metadata.comparison_history {
                anyhow::ensure!(
                    comparison.alternatives.len() == 2
                        && comparison.alternatives[0].id == "A"
                        && comparison.alternatives[1].id == "B"
                        && ["A", "B"].contains(&comparison.recommendation.plan_id.as_str()),
                    "Malformed comparison history"
                );
            }
            found = Some(metadata);
        }
        Ok(found)
    }

    pub fn require_markdown(markdown: &str) -> anyhow::Result<Self> {
        let identity = ArtifactIdentity::from_markdown(markdown)?
            .ok_or_else(|| anyhow::anyhow!("Change specification has no stable identity"))?;
        let metadata = Self::from_markdown(markdown)?
            .ok_or_else(|| anyhow::anyhow!("Change specification has no structured status"))?;
        anyhow::ensure!(
            metadata.uid == identity.uid && metadata.display_id == identity.display_id,
            "Change status identity does not match the change specification"
        );
        Ok(metadata)
    }

    pub fn parse_legacy_markdown(markdown: &str) -> anyhow::Result<ChangeStatus> {
        let lines = markdown
            .lines()
            .filter_map(|line| line.strip_prefix(STATUS_PREFIX).map(str::trim))
            .collect::<Vec<_>>();
        anyhow::ensure!(
            lines.len() == 1,
            "Legacy change requires exactly one visible status line"
        );
        ChangeStatus::parse_legacy(lines[0])
    }

    pub fn strip_markers(markdown: &str) -> String {
        let mut result = markdown
            .lines()
            .filter(|line| !line.starts_with(MARKER))
            .collect::<Vec<_>>()
            .join("\n");
        if markdown.ends_with('\n') {
            result.push('\n');
        }
        result
    }

    /// Remove model-supplied metadata, insert the application-owned record,
    /// and render the visible status from the typed value.
    pub fn write_markdown(
        markdown: &str,
        identity: &ArtifactIdentity,
        status: ChangeStatus,
    ) -> anyhow::Result<String> {
        let clean = Self::strip_markers(markdown);
        let mut lines = clean.lines().map(str::to_owned).collect::<Vec<_>>();
        let mut rendered = false;
        for line in &mut lines {
            if line.starts_with(STATUS_PREFIX) {
                anyhow::ensure!(!rendered, "Change has multiple visible status lines");
                *line = render_status_line(line, status);
                rendered = true;
            }
        }
        if !rendered {
            lines.insert(
                status_line_index(&lines),
                format!("{STATUS_PREFIX} {}", status.label()),
            );
        }
        let mut metadata = Self::from_markdown(markdown)?
            .filter(|existing| {
                existing.uid == identity.uid && existing.display_id == identity.display_id
            })
            .unwrap_or_else(|| Self::new(identity, status));
        if metadata.schema_version == 1
            && metadata.status == ChangeStatus::Draft
            && status == ChangeStatus::Ready
        {
            metadata.schema_version = 2;
        }
        metadata.status = status;
        let marker = format!("{MARKER}{} -->", serde_json::to_string(&metadata)?);
        let identity_marker = "<!-- packet-artifact-id:v1 ";
        let index = lines
            .iter()
            .position(|line| line.starts_with(identity_marker))
            .map(|index| index + 1)
            .or_else(|| {
                lines
                    .iter()
                    .position(|line| line.starts_with("# "))
                    .map(|i| i + 1)
            })
            .unwrap_or(0);
        lines.insert(index, marker);
        if lines
            .get(index + 1)
            .is_some_and(|line| !line.trim().is_empty())
        {
            lines.insert(index + 1, String::new());
        }
        let mut result = lines.join("\n");
        if markdown.ends_with('\n') {
            result.push('\n');
        }
        Ok(result)
    }

    pub fn render_status(markdown: &str, status: ChangeStatus) -> anyhow::Result<String> {
        let mut lines = markdown.lines().map(str::to_owned).collect::<Vec<_>>();
        let mut found = false;
        for line in &mut lines {
            if line.starts_with(STATUS_PREFIX) {
                anyhow::ensure!(!found, "Change has multiple visible status lines");
                *line = render_status_line(line, status);
                found = true;
            }
        }
        if !found {
            lines.insert(
                status_line_index(&lines),
                format!("{STATUS_PREFIX} {}", status.label()),
            );
        }
        let mut rendered = lines.join("\n");
        if markdown.ends_with('\n') {
            rendered.push('\n');
        }
        Ok(rendered)
    }
}

pub(super) fn replace_metadata(
    markdown: &str,
    metadata: &ChangeMetadata,
) -> anyhow::Result<String> {
    let marker = format!("{MARKER}{} -->", serde_json::to_string(metadata)?);
    let mut found = false;
    let mut result = String::new();
    for line in markdown.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if bare.starts_with(MARKER) {
            anyhow::ensure!(!found, "Markdown contains duplicate change metadata");
            result.push_str(&marker);
            if line.ends_with('\n') {
                result.push('\n');
            }
            found = true;
        } else {
            result.push_str(line);
        }
    }
    anyhow::ensure!(found, "Change specification has no structured status");
    Ok(result)
}

fn status_line_index(lines: &[String]) -> usize {
    lines
        .iter()
        .position(|line| line.starts_with("<!-- packet-artifact-id:v1 "))
        .map(|index| index + 1)
        .or_else(|| {
            lines
                .iter()
                .position(|line| line.starts_with("# "))
                .map(|i| i + 1)
        })
        .unwrap_or(0)
}

fn render_status_line(line: &str, status: ChangeStatus) -> String {
    let value = line.strip_prefix(STATUS_PREFIX).unwrap_or_default().trim();
    let suffix = ChangeStatus::ALL
        .iter()
        .find_map(|known| {
            let suffix = value.strip_prefix(known.label())?;
            let suffix = suffix.trim_start();
            (suffix.is_empty()
                || suffix.starts_with('—')
                || suffix.starts_with('-')
                || suffix.starts_with(':')
                || suffix.starts_with('('))
            .then_some(suffix)
        })
        .unwrap_or("");
    format!(
        "{STATUS_PREFIX} {}{}",
        status.label(),
        if suffix.is_empty() { "" } else { " " }
    ) + suffix
}
