use super::*;

impl ComputerActionV1 {
    /// Bind an untrusted action request to the actor authenticated by the
    /// controller ingress. Never take the actor value as proof of identity.
    pub fn validate_as(
        &self,
        authenticated_actor: ComputerActionActorV1,
        session: &super::super::ComputerSessionV1,
        now_unix_ms: u64,
    ) -> Result<(), &'static str> {
        if self.actor != authenticated_actor {
            return Err("action actor does not match authenticated caller");
        }
        self.validate(session, now_unix_ms)
    }

    pub fn validate(
        &self,
        session: &super::super::ComputerSessionV1,
        now_unix_ms: u64,
    ) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported action version");
        }
        session.lease.validate(now_unix_ms)?;
        let policy = &session.policy;
        policy.validate()?;
        if session.session_started_at_unix_ms == 0
            || session.session_started_at_unix_ms > session.lease.issued_at_unix_ms
        {
            return Err("session start time is invalid");
        }
        if now_unix_ms.saturating_sub(session.session_started_at_unix_ms)
            >= policy.resource_budget.time_limit_ms
        {
            return Err("session action budget has expired");
        }
        if !is_uuid(&self.session_id) || !is_uuid(&self.idempotency_key) {
            return Err("invalid action identity");
        }
        if self.session_id != session.identity.session_id
            || self.lease_generation != session.lease.generation
            || self.approval_generation != policy.approval_generation
            || self.cancellation_generation != session.cancellation_generation
            || self.action_sequence > policy.resource_budget.action_limit as u64
        {
            return Err("action scope or approval generation mismatch");
        }
        let actor_owns_control = matches!(
            (self.actor, session.lifecycle),
            (
                ComputerActionActorV1::Agent,
                super::super::ComputerLifecycleV1::Delegated
            ) | (
                ComputerActionActorV1::Human,
                super::super::ComputerLifecycleV1::HumanControl
            )
        );
        if !actor_owns_control {
            return Err("action actor does not own computer control");
        }
        if self.action_sequence == 0
            || self.approval_generation == 0
            || self.cancellation_generation == 0
        {
            return Err("action generations and sequence must be positive");
        }
        if !policy.authorized_operations.contains(&self.operation) {
            return Err("action operation is not authorized");
        }
        let payload_matches = matches!(
            (&self.operation, &self.payload),
            (
                ComputerOperationV1::ObserveScreen,
                ComputerActionPayloadV1::ObserveScreen
            ) | (
                ComputerOperationV1::ObserveWindows,
                ComputerActionPayloadV1::ObserveWindows
            ) | (
                ComputerOperationV1::ReadAccessibility,
                ComputerActionPayloadV1::ReadAccessibility
            ) | (
                ComputerOperationV1::MouseInput,
                ComputerActionPayloadV1::MouseClick { .. }
            ) | (
                ComputerOperationV1::MouseInput,
                ComputerActionPayloadV1::MouseDoubleClick { .. }
            ) | (
                ComputerOperationV1::MouseInput,
                ComputerActionPayloadV1::MouseMove { .. }
            ) | (
                ComputerOperationV1::MouseInput,
                ComputerActionPayloadV1::MouseDrag { .. }
            ) | (
                ComputerOperationV1::MouseInput,
                ComputerActionPayloadV1::MouseScroll { .. }
            ) | (
                ComputerOperationV1::MouseInput,
                ComputerActionPayloadV1::FocusWindow { .. }
            ) | (
                ComputerOperationV1::KeyboardInput,
                ComputerActionPayloadV1::KeyPress { .. }
            ) | (
                ComputerOperationV1::KeyboardInput,
                ComputerActionPayloadV1::KeyChord { .. }
            ) | (
                ComputerOperationV1::KeyboardInput,
                ComputerActionPayloadV1::TypeText { .. }
            ) | (
                ComputerOperationV1::LaunchAllowedApp,
                ComputerActionPayloadV1::LaunchApp { .. }
            ) | (
                ComputerOperationV1::WaitFor,
                ComputerActionPayloadV1::WaitFor { .. }
            ) | (
                ComputerOperationV1::QueryState,
                ComputerActionPayloadV1::QueryState
            )
        );
        if !payload_matches {
            return Err("action payload does not match its operation");
        }
        match &self.payload {
            ComputerActionPayloadV1::MouseClick { target, x, y, .. }
            | ComputerActionPayloadV1::MouseDoubleClick { target, x, y, .. }
            | ComputerActionPayloadV1::MouseMove { target, x, y }
            | ComputerActionPayloadV1::MouseScroll { target, x, y, .. }
                if target.validate_point(*x, *y).is_err() =>
            {
                return Err("pointer action does not match its observed frame and window");
            }
            ComputerActionPayloadV1::MouseDrag {
                target,
                from_x,
                from_y,
                to_x,
                to_y,
                ..
            } if target.validate_point(*from_x, *from_y).is_err()
                || target.validate_point(*to_x, *to_y).is_err() =>
            {
                return Err("pointer action does not match its observed frame and window");
            }
            ComputerActionPayloadV1::FocusWindow {
                window_id,
                observation_revision,
            }
            | ComputerActionPayloadV1::KeyPress {
                window_id,
                observation_revision,
                ..
            }
            | ComputerActionPayloadV1::KeyChord {
                window_id,
                observation_revision,
                ..
            }
            | ComputerActionPayloadV1::TypeText {
                window_id,
                observation_revision,
                ..
            } if !is_uuid(window_id) || *observation_revision == 0 => {
                return Err("window action is not bound to an observation");
            }
            ComputerActionPayloadV1::KeyChord { keys, .. } if keys.is_empty() || keys.len() > 8 => {
                return Err("key chord exceeds bounds");
            }
            ComputerActionPayloadV1::TypeText { text, .. }
                if text.len() > 4096 || text.contains('\0') =>
            {
                return Err("text input exceeds bounds");
            }
            ComputerActionPayloadV1::WaitFor {
                condition,
                timeout_ms,
            } if !(1..=30_000).contains(timeout_ms) || condition.validate().is_err() => {
                return Err("wait request exceeds bounds");
            }
            ComputerActionPayloadV1::MouseScroll {
                delta_x, delta_y, ..
            } if (*delta_x == 0 && *delta_y == 0)
                || delta_x.unsigned_abs() > 30
                || delta_y.unsigned_abs() > 30 =>
            {
                return Err("scroll delta exceeds bounds");
            }
            ComputerActionPayloadV1::LaunchApp { app_id }
                if app_id.is_empty()
                    || app_id.len() > 64
                    || !app_id.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'
                    }) =>
            {
                return Err("app identifier is invalid");
            }
            ComputerActionPayloadV1::LaunchApp { app_id }
                if !policy.allowed_apps.contains(app_id) =>
            {
                return Err("app is not in the session allowlist");
            }
            _ => {}
        }
        Ok(())
    }
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

#[cfg(test)]
mod tests;
