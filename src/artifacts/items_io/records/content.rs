use crate::domain::{DecisionBrief, OpenItem};
use serde::{Deserialize, Serialize};

const CONTENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Header {
    schema_version: u32,
    uid: String,
    id: String,
    decision_brief: Option<DecisionBrief>,
}

#[derive(Debug, Clone)]
pub(super) struct ItemContent {
    pub uid: String,
    pub id: String,
    pub question: String,
    pub reason: String,
    pub recommendation: String,
    pub evidence: String,
    pub decision_brief: Option<DecisionBrief>,
}

pub(super) fn serialize_content(item: &OpenItem) -> anyhow::Result<String> {
    let uid = item
        .uid
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Open item {} has no stable UID", item.id))?;
    let header = Header {
        schema_version: CONTENT_SCHEMA_VERSION,
        uid: uid.to_owned(),
        id: item.id.clone(),
        decision_brief: item.decision_brief.clone(),
    };
    let header = serde_json::to_string(&header)?;
    Ok(format!(
        "---\nkoolade-item-content: {header}\n---\n\n# {}\n\n{}{}{}{}",
        item.id,
        section("Question", "question", &item.question),
        section("Reason", "reason", &item.reason),
        section("Recommendation", "recommendation", &item.recommendation),
        section("Evidence", "evidence", &item.evidence),
    ))
}

fn section(heading: &str, key: &str, value: &str) -> String {
    format!(
        "## {heading}\n\n<!-- koolade-item-{key} -->\n{}\n<!-- /koolade-item-{key} -->\n\n",
        value.trim()
    )
}

pub(super) fn parse_content(markdown: &str) -> anyhow::Result<ItemContent> {
    let mut lines = markdown.lines();
    anyhow::ensure!(
        lines.next() == Some("---"),
        "Item content has no front matter"
    );
    let encoded = lines
        .next()
        .and_then(|line| line.strip_prefix("koolade-item-content: "))
        .ok_or_else(|| anyhow::anyhow!("Item content front matter is malformed"))?;
    let header: Header = serde_json::from_str(encoded)?;
    anyhow::ensure!(
        header.schema_version == CONTENT_SCHEMA_VERSION,
        "Unsupported item content schema version {}",
        header.schema_version
    );
    anyhow::ensure!(
        uuid::Uuid::parse_str(&header.uid).is_ok(),
        "Item content UID is invalid"
    );
    anyhow::ensure!(
        crate::core::ids::is_valid_id(&header.id),
        "Item content ID is invalid"
    );
    anyhow::ensure!(
        lines.next() == Some("---"),
        "Item content front matter is not closed"
    );
    let body = lines.collect::<Vec<_>>().join("\n");
    let question = extract(&body, "question")?;
    anyhow::ensure!(
        !question.trim().is_empty(),
        "Item content question is empty"
    );
    let reason = extract(&body, "reason")?;
    let recommendation = extract(&body, "recommendation")?;
    let evidence = extract(&body, "evidence")?;
    let mut decision_brief = header.decision_brief;
    if let Some(brief) = &mut decision_brief {
        brief.bind_to_item(&header.id).map_err(anyhow::Error::msg)?;
    }
    Ok(ItemContent {
        uid: uuid::Uuid::parse_str(&header.uid)?.hyphenated().to_string(),
        id: header.id,
        question,
        reason,
        recommendation,
        evidence,
        decision_brief,
    })
}

fn extract(body: &str, key: &str) -> anyhow::Result<String> {
    let open = format!("<!-- koolade-item-{key} -->");
    let close = format!("<!-- /koolade-item-{key} -->");
    anyhow::ensure!(
        body.matches(&open).count() == 1 && body.matches(&close).count() == 1,
        "Item content section {key} is missing or repeated"
    );
    let value = body
        .split_once(&open)
        .and_then(|(_, rest)| rest.split_once(&close).map(|(value, _)| value))
        .ok_or_else(|| anyhow::anyhow!("Item content section {key} is malformed"))?;
    Ok(value.trim_matches('\n').to_owned())
}
