use crate::domain::{Authority, ItemKind, ItemStatus, OpenItem, Priority};
use serde::{Deserialize, Serialize};

pub(super) const RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ItemRecord {
    pub schema_version: u32,
    pub uid: String,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub data: ItemState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ItemState {
    pub id: String,
    pub priority: Priority,
    pub authority: Authority,
    pub kind: ItemKind,
    pub category: String,
    pub assigned_to: Option<String>,
    pub conversation_id: Option<String>,
    pub feature_id: Option<String>,
    pub feature_uid: Option<String>,
    pub blocked_by: Vec<String>,
    pub status: ItemStatus,
}

#[derive(Debug, Clone)]
pub(super) struct LoadedItem {
    pub record: ItemRecord,
    pub item: OpenItem,
}

impl From<&OpenItem> for ItemState {
    fn from(item: &OpenItem) -> Self {
        Self {
            id: item.id.clone(),
            priority: item.priority,
            authority: item.authority,
            kind: item.kind,
            category: item.category.clone(),
            assigned_to: item.assigned_to.clone(),
            conversation_id: item.conversation_id.clone(),
            feature_id: item.feature_id.clone(),
            feature_uid: item.feature_uid.clone(),
            blocked_by: item.blocked_by.clone(),
            status: item.status,
        }
    }
}

impl ItemRecord {
    pub fn item(&self, content: super::content::ItemContent) -> OpenItem {
        let mut item = OpenItem {
            uid: Some(self.uid.clone()),
            id: self.data.id.clone(),
            conversation_id: self.data.conversation_id.clone(),
            priority: self.data.priority,
            authority: self.data.authority,
            kind: self.data.kind,
            category: self.data.category.clone(),
            assigned_to: self.data.assigned_to.clone(),
            question: content.question,
            reason: content.reason,
            feature_id: self.data.feature_id.clone(),
            feature_uid: self.data.feature_uid.clone(),
            recommendation: content.recommendation,
            evidence: content.evidence,
            decision_brief: content.decision_brief,
            blocked_by: self.data.blocked_by.clone(),
            status: self.data.status,
            record_revision: self.revision,
            record_baseline: None,
        };
        item.record_baseline = serde_json::to_string(&item).ok();
        item
    }
}

impl LoadedItem {
    pub fn new(record: ItemRecord, content: super::content::ItemContent) -> Self {
        let item = record.item(content);
        Self { record, item }
    }
}
