//! Strict parser for the Koolade-owned open-items Markdown representation.
use crate::domain::{Authority, DecisionBrief, ItemKind, ItemStatus, OpenItem, Priority};

/// Parse the artifact back into the structured queue (only Open items ever
/// reach the file; the parser stamps `Open`).
pub fn parse(text: &str) -> Result<Vec<OpenItem>, String> {
    let mut items = Vec::new();
    let mut cur: Option<Partial> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            if let Some(previous) = cur.take() {
                previous.finish(&mut items)?;
            }
            let id = rest.trim().to_owned();
            if !crate::core::ids::is_valid_id(&id) {
                return Err(format!(
                    "malformed item heading '## {rest}' (expected e.g. CLR-012)"
                ));
            }
            cur = Some(Partial::new(id));
            continue;
        }
        let Some(partial) = cur.as_mut() else {
            continue;
        };
        if trimmed.is_empty() {
            continue;
        }
        if let Some(section) = trimmed.strip_prefix("### ") {
            partial.enter_section(section.trim());
        } else if partial.mode() == Capture::Field {
            if let Some((key, value)) = bold_field(trimmed) {
                partial.field(key, value)?;
            }
        } else if let Some(buffer) = partial.capture_buffer() {
            push_line(buffer, line);
        }
    }
    if let Some(previous) = cur {
        previous.finish(&mut items)?;
    }
    Ok(items)
}

fn push_line(buffer: &mut String, line: &str) {
    if !buffer.is_empty() {
        buffer.push('\n');
    }
    buffer.push_str(line.trim_end());
}

fn bold_field(line: &str) -> Option<(&str, &str)> {
    let inner = line.strip_prefix("**")?;
    let index = inner.find("**")?;
    Some((
        inner[..index].trim().trim_end_matches(':'),
        inner[index + 2..].trim(),
    ))
}

struct Partial {
    id: String,
    uid: Option<String>,
    conversation_id: Option<String>,
    priority: Option<Priority>,
    authority: Option<Authority>,
    kind: Option<ItemKind>,
    category: Option<String>,
    assigned_to: Option<String>,
    _status: Option<ItemStatus>,
    mode: Capture,
    question_buf: String,
    reason_buf: String,
    feature_id: Option<String>,
    feature_uid: Option<String>,
    recommendation_buf: String,
    evidence_buf: String,
    decision_brief: Option<DecisionBrief>,
    blocked_by: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Capture {
    Field,
    Question,
    Reason,
    Recommendation,
    Evidence,
    Ignore,
}

impl Partial {
    fn new(id: String) -> Self {
        Self {
            id,
            uid: None,
            conversation_id: None,
            priority: None,
            authority: None,
            kind: None,
            category: None,
            assigned_to: None,
            _status: None,
            mode: Capture::Field,
            question_buf: String::new(),
            reason_buf: String::new(),
            feature_id: None,
            feature_uid: None,
            recommendation_buf: String::new(),
            evidence_buf: String::new(),
            decision_brief: None,
            blocked_by: Vec::new(),
        }
    }

    fn enter_section(&mut self, name: &str) {
        self.mode = match name {
            "Question" => Capture::Question,
            "Reason" => Capture::Reason,
            "Recommendation" => Capture::Recommendation,
            "Evidence" => Capture::Evidence,
            _ => Capture::Ignore,
        };
    }

    fn capture_buffer(&mut self) -> Option<&mut String> {
        match self.mode {
            Capture::Question => Some(&mut self.question_buf),
            Capture::Reason => Some(&mut self.reason_buf),
            Capture::Recommendation => Some(&mut self.recommendation_buf),
            Capture::Evidence => Some(&mut self.evidence_buf),
            _ => None,
        }
    }

    const fn mode(&self) -> Capture {
        self.mode
    }

    fn field(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "UID" => {
                let uid = uuid::Uuid::parse_str(value)
                    .map_err(|_| format!("bad 'UID' value: '{value}'"))?;
                self.uid = Some(uid.hyphenated().to_string());
            }
            "Priority" => {
                self.priority = Some(Priority::parse_i(value).ok_or_else(|| bad_value(key, value))?)
            }
            "Authority" => {
                self.authority =
                    Some(Authority::parse_i(value).ok_or_else(|| bad_value(key, value))?)
            }
            "Type" => {
                self.kind = Some(ItemKind::parse_i(value).ok_or_else(|| bad_value(key, value))?)
            }
            "Status" => {
                self._status =
                    Some(ItemStatus::parse_i(value).ok_or_else(|| bad_value(key, value))?)
            }
            "Category" => self.category = Some(value.to_owned()),
            "Feature" => self.feature_id = Some(value.to_owned()),
            "Feature UID" => {
                let uid = uuid::Uuid::parse_str(value)
                    .map_err(|_| format!("bad 'Feature UID' value: '{value}'"))?;
                self.feature_uid = Some(uid.hyphenated().to_string());
            }
            "Blocked By" => {
                self.blocked_by = value
                    .split(',')
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .collect();
                if self
                    .blocked_by
                    .iter()
                    .any(|id| !crate::core::ids::is_valid_id(id))
                {
                    return Err(format!("bad 'Blocked By' value: '{value}'"));
                }
            }
            "Conversation" => self.conversation_id = Some(value.to_owned()),
            "Decision Brief" => {
                if value.len() > 48_000 {
                    return Err("decision brief exceeds the storage limit".into());
                }
                self.decision_brief = Some(
                    serde_json::from_str(value)
                        .map_err(|error| format!("bad 'Decision Brief' value: {error}"))?,
                );
            }
            "Assigned To" => {
                self.assigned_to = (value != "(unassigned)").then(|| value.to_owned());
            }
            other => return Err(format!("unknown item field '{other}'")),
        }
        Ok(())
    }

    fn finish(self, items: &mut Vec<OpenItem>) -> Result<(), String> {
        if let Some(uid) = &self.uid
            && items
                .iter()
                .any(|existing| existing.uid.as_deref() == Some(uid))
        {
            return Err(format!("duplicate open item UID {uid}"));
        }
        let question = self.question_buf.trim().to_owned();
        if question.is_empty() {
            return Err(format!("{}: missing '### Question' section", self.id));
        }
        let priority = self
            .priority
            .ok_or_else(|| format!("{}: missing '**Priority:**'", self.id))?;
        let kind = self
            .kind
            .ok_or_else(|| format!("{}: missing '**Type:**'", self.id))?;
        let category = self
            .category
            .filter(|category| !category.trim().is_empty())
            .ok_or_else(|| format!("{}: missing '**Category:**'", self.id))?;
        let mut item = OpenItem::new(
            self.id,
            priority,
            kind,
            category,
            self.assigned_to,
            question,
            self.reason_buf.trim().to_owned(),
        );
        if items.iter().any(|existing| existing.id == item.id) {
            return Err(format!("duplicate open item id {}", item.id));
        }
        item.uid = self.uid;
        item.authority = self.authority.unwrap_or(Authority::Human);
        item.conversation_id = self.conversation_id;
        item.feature_id = self.feature_id;
        item.feature_uid = self.feature_uid;
        item.blocked_by = self.blocked_by;
        item.recommendation = self.recommendation_buf.trim().to_owned();
        item.evidence = self.evidence_buf.trim().to_owned();
        if let Some(mut brief) = self.decision_brief {
            brief
                .bind_to_item(&item.id)
                .map_err(|error| format!("{}: {error}", item.id))?;
            item.decision_brief = Some(brief);
        }
        items.push(item);
        Ok(())
    }
}

fn bad_value(key: &str, value: &str) -> String {
    format!("bad '{key}' value: '{value}'")
}
