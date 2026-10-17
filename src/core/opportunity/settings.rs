use serde::{Deserialize, Serialize};

/// Workspace-local controls for bounded proactive exploration. Disabled by
/// default: a tagged ScheduledExploration proposal is invalid until the user
/// opts in through an explicit settings action.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpportunityDiscoverySettingsV1 {
    pub schema_version: u16,
    pub exploration: OpportunityExploreModeV1,
    /// Maximum advisory proposals in one UTC calendar day. This limits output,
    /// not user decisions or implementation work.
    pub daily_proposal_limit: u8,
    /// Maximum concurrent Kool.ad/e self-improvement runs requiring a user
    /// decision, planning, implementation, review, or recovery action.
    #[serde(default = "default_active_self_improvement_limit")]
    pub active_self_improvement_limit: u8,
    #[serde(default)]
    pub last_exploration_at_ms: Option<u64>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityExploreModeV1 {
    Disabled,
    OptedIn,
}

impl Default for OpportunityDiscoverySettingsV1 {
    fn default() -> Self {
        Self {
            schema_version: 1,
            exploration: OpportunityExploreModeV1::Disabled,
            daily_proposal_limit: 3,
            active_self_improvement_limit: default_active_self_improvement_limit(),
            last_exploration_at_ms: None,
            revision: 0,
        }
    }
}

impl OpportunityDiscoverySettingsV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !(1..=10).contains(&self.daily_proposal_limit)
            || !(1..=10).contains(&self.active_self_improvement_limit)
            || self.last_exploration_at_ms == Some(0)
        {
            return Err("opportunity discovery settings are invalid".into());
        }
        Ok(())
    }
}

fn default_active_self_improvement_limit() -> u8 {
    2
}
