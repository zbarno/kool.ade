use serde::{Deserialize, Serialize};

/// A controller-issued lease. A guest or model may present but never renew it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerLeaseV1 {
    pub schema_version: u16,
    pub owner_id: String,
    pub lease_id: String,
    pub generation: u64,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub cancellation_generation: u64,
}

impl ComputerLeaseV1 {
    pub fn validate(&self, now_unix_ms: u64) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported lease version");
        }
        if !is_uuid(&self.owner_id) || !is_uuid(&self.lease_id) {
            return Err("invalid lease identity");
        }
        if self.generation == 0 || self.cancellation_generation == 0 {
            return Err("lease generations must be positive");
        }
        if self.issued_at_unix_ms > now_unix_ms || self.expires_at_unix_ms <= now_unix_ms {
            return Err("lease is not currently valid");
        }
        if self
            .expires_at_unix_ms
            .saturating_sub(self.issued_at_unix_ms)
            > 15 * 60 * 1_000
        {
            return Err("lease duration exceeds the controller limit");
        }
        Ok(())
    }

    /// A renewal must preserve owner authority and strictly advance its fence.
    pub fn validate_renewal_from(&self, previous: &Self) -> Result<(), &'static str> {
        if self.owner_id != previous.owner_id || self.lease_id != previous.lease_id {
            return Err("lease renewal identity changed");
        }
        if Some(self.generation) != previous.generation.checked_add(1)
            || self.cancellation_generation < previous.cancellation_generation
            || self.issued_at_unix_ms < previous.issued_at_unix_ms
            || self.expires_at_unix_ms <= previous.expires_at_unix_ms
        {
            return Err("lease renewal did not advance monotonically");
        }
        Ok(())
    }
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
