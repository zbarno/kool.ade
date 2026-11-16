use crate::{
    computer_use::{
        ComputerActionActorV1 as Actor, ComputerActionPayloadV1 as Payload,
        ComputerOperationV1 as Operation, ComputerSessionV1,
        artifacts::ComputerArtifactBridgeV1,
        guest_protocol::{
            AuthenticatedGuestChannelV1, AuthorizedComputerUseSessionV1, GuestActionJournalV1,
        },
    },
    runtime_qa::{
        QaComputerTargetV1, QaDriverActionV1, QaRunRequestV1,
        drivers::computer_vm::{ComputerQaDisplayV1, ComputerQaGuestSessionV1},
    },
};
use base64::Engine as _;
use ed25519_dalek::SigningKey;
use std::time::{Duration, Instant};

pub(super) struct LiveQaGuest<'a> {
    pub(super) session: &'a ComputerSessionV1,
    pub(super) actor: Actor,
    pub(super) controller_key_id: &'a str,
    pub(super) controller_key: &'a SigningKey,
    pub(super) authority: &'a AuthorizedComputerUseSessionV1,
    pub(super) channel: &'a mut AuthenticatedGuestChannelV1,
    pub(super) journal: &'a GuestActionJournalV1,
    pub(super) bridge: &'a ComputerArtifactBridgeV1,
    pub(super) next_sequence: u64,
    pub(super) fixture: std::path::PathBuf,
    pub(super) request: Option<QaRunRequestV1>,
    pub(super) preexisting_windows: std::collections::BTreeSet<String>,
    pub(super) latest: Option<crate::computer_use::guest_protocol::GuestObservationV1>,
}

impl LiveQaGuest<'_> {
    fn observe(
        &mut self,
    ) -> Result<crate::computer_use::guest_protocol::GuestObservationV1, String> {
        let action = super::super::action_as(
            self.session,
            self.actor,
            self.next_sequence,
            Operation::ObserveScreen,
            Payload::ObserveScreen,
        );
        self.next_sequence += 1;
        let result = self
            .authority
            .execute(
                self.channel,
                self.journal,
                &action,
                super::super::unix_time_ms()?.saturating_add(60_000),
                super::super::unix_time_ms()?,
            )
            .map_err(|error| format!("real QA guest observation: {error:?}"))?;
        let observation = result
            .observation
            .ok_or("real QA guest observation returned no screen")?;
        self.latest = Some(observation.clone());
        Ok(observation)
    }

    fn active_window(
        &self,
    ) -> Result<crate::computer_use::guest_protocol::GuestWindowObservationV1, String> {
        self.latest
            .as_ref()
            .and_then(|observation| {
                observation
                    .windows
                    .iter()
                    .find(|window| window.focused)
                    .or_else(|| observation.windows.last())
            })
            .cloned()
            .ok_or_else(|| {
                "real QA fixture window was absent from authenticated guest observation".into()
            })
    }
}

impl ComputerQaGuestSessionV1 for LiveQaGuest<'_> {
    fn begin_scenario(
        &mut self,
        target: &QaComputerTargetV1,
        _scenario_id: &str,
        deadline: Instant,
    ) -> Result<ComputerQaDisplayV1, String> {
        if Instant::now() >= deadline {
            return Err("real QA scenario timed out before starting".into());
        }
        let observation = self.observe()?;
        self.preexisting_windows = observation
            .windows
            .iter()
            .map(|window| window.window_id.clone())
            .collect();
        Ok(ComputerQaDisplayV1 {
            width: observation.width_px as u16,
            height: observation.height_px as u16,
            dpi: (96_u32.saturating_mul(observation.device_scale_numerator)
                / observation.device_scale_denominator.max(1)) as u16,
        })
        .and_then(|display| {
            if display.width == target.display_width
                && display.height == target.display_height
                && display.dpi == target.display_dpi
            {
                Ok(display)
            } else {
                Err("real QA guest display differed from the approved target".into())
            }
        })
    }

    fn launch_staged_application(
        &mut self,
        target: &QaComputerTargetV1,
        _deadline: Instant,
    ) -> Result<(), String> {
        self.stage_and_launch(target)
    }

    fn observe_state(&mut self, _deadline: Instant) -> Result<String, String> {
        let observation = self.observe()?;
        Ok(observation
            .windows
            .iter()
            .map(|window| window.title.clone())
            .collect::<Vec<_>>()
            .join(" | "))
    }

    fn perform_action(
        &mut self,
        action: &QaDriverActionV1,
        deadline: Instant,
    ) -> Result<(), String> {
        if matches!(action, QaDriverActionV1::WaitForWindow) {
            loop {
                let observation = self.observe()?;
                if observation
                    .windows
                    .iter()
                    .any(|window| !self.preexisting_windows.contains(&window.window_id))
                {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(
                        "real QA scenario timed out waiting for its launched app window".into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let QaDriverActionV1::Click { x, y, .. } = action else {
            return Err("real QA KVM fixture only accepts wait and click actions".into());
        };
        let observation = self
            .latest
            .as_ref()
            .ok_or("real QA click requires a fresh guest observation")?;
        let window = self.active_window()?;
        let target = crate::computer_use::ComputerFrameReferenceV1 {
            frame_id: observation.frame_id.clone(),
            display_generation: observation.display_generation,
            observation_revision: observation.observation_revision,
            display_width_px: observation.width_px,
            display_height_px: observation.height_px,
            device_scale_numerator: observation.device_scale_numerator,
            device_scale_denominator: observation.device_scale_denominator,
            window_id: window.window_id.clone(),
            window_generation: window.generation,
            window_bounds: window.bounds.clone(),
        };
        let payload = Payload::MouseClick {
            x: (window.bounds.left.max(0) as u32).saturating_add(u32::from(*x)),
            y: (window.bounds.top.max(0) as u32).saturating_add(u32::from(*y)),
            target,
            button: crate::computer_use::MouseButtonV1::Primary,
        };
        let action = super::super::action_as(
            self.session,
            self.actor,
            self.next_sequence,
            Operation::MouseInput,
            payload,
        );
        self.next_sequence += 1;
        let result = self
            .authority
            .execute(
                self.channel,
                self.journal,
                &action,
                super::super::unix_time_ms()?.saturating_add(60_000),
                super::super::unix_time_ms()?,
            )
            .map_err(|error| format!("real QA guest click: {error:?}"))?;
        super::super::require_guest_action_completed(
            "real QA fixture click",
            &result.reply.result,
        )?;
        std::thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    fn screenshot_png(&mut self, _deadline: Instant) -> Result<Vec<u8>, String> {
        let observation = self
            .latest
            .as_ref()
            .ok_or("real QA screenshot requires a fresh guest frame")?;
        base64::engine::general_purpose::STANDARD
            .decode(&observation.screenshot_png_base64)
            .map_err(|_| "real QA guest screenshot was not valid base64".into())
    }

    fn end_scenario(&mut self) -> Result<(), String> {
        let observation = self
            .latest
            .as_ref()
            .ok_or("real QA cleanup requires the current guest frame")?;
        let window = self.active_window()?;
        let action = super::super::action_as(
            self.session,
            self.actor,
            self.next_sequence,
            Operation::KeyboardInput,
            Payload::KeyPress {
                key: crate::computer_use::ComputerKeyV1::Named(
                    crate::computer_use::KeyCodeV1::Escape,
                ),
                window_id: window.window_id,
                observation_revision: observation.observation_revision,
            },
        );
        self.next_sequence += 1;
        self.authority
            .execute(
                self.channel,
                self.journal,
                &action,
                super::super::unix_time_ms()?.saturating_add(60_000),
                super::super::unix_time_ms()?,
            )
            .map_err(|error| format!("real QA guest fixture cleanup: {error:?}"))?;
        Ok(())
    }
}
