use super::{QaDriverActionV1, QaScenarioStepV1};

pub(super) fn step(
    driver_action: QaDriverActionV1,
    expected_state: &str,
    screenshot_checkpoint: bool,
) -> QaScenarioStepV1 {
    QaScenarioStepV1 {
        driver_action,
        expected_state: Some(expected_state.into()),
        screenshot_checkpoint,
    }
}
