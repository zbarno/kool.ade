use crate::runtime_qa::drivers::computer_vm::{
    ComputerQaDisplayV1, ComputerQaGuestSessionV1, ComputerQaScenarioDriverV1,
};
use crate::runtime_qa::{
    QaAssertionStatus, QaComputerTargetV1, QaDriverActionV1, QaDriverOutputV1, QaPlatform,
    QaRunRequestV1, QaScenarioDriver, QaScenarioStepV1, QaScenarioV1, QaTargetAuthorization,
};
use std::time::Instant;

/// Production boundary: the Computer VM adapter converts approved typed
/// desktop scenarios into guest actions and Runtime QA assertions. Contract
/// unit tests cannot prove launch/action/observation/evidence sequencing as a
/// whole. This uses an in-process guest fixture; the opt-in KVM acceptance
/// separately covers the real QEMU guest transport. No Docker or network is
/// required; setup cost is low.
#[test]
fn computer_vm_driver_records_seeded_defect_and_corrected_retest() {
    let (mut request, mut scenarios, _) = super::super::support::fixture();
    super::bind_build_hash(&mut request);
    request.relevant_platforms = vec![QaPlatform::LinuxDesktop];
    let scenario = &mut scenarios[0];
    scenario.visual_expectations = vec!["approved app window is visible".into()];
    scenario.steps = vec![
        QaScenarioStepV1 {
            driver_action: QaDriverActionV1::LaunchValidatedTarget,
            expected_state: Some("approved app visible".into()),
            screenshot_checkpoint: false,
        },
        QaScenarioStepV1 {
            driver_action: QaDriverActionV1::Click {
                window_id: "main-window".into(),
                x: 40,
                y: 24,
            },
            expected_state: Some("destination /expected".into()),
            screenshot_checkpoint: true,
        },
    ];
    let target = super::make_target(&request);
    let authorization = QaTargetAuthorization::ComputerVm(&target);
    let mut guest = SeededDesktopGuest {
        wrong_destination: true,
        ..SeededDesktopGuest::default()
    };

    let seeded = run_scenario(&request, scenario, &target, &mut guest, &authorization);
    assert!(seeded.completed, "scenario execution should complete");
    assert_eq!(seeded.actions.len(), 2);
    assert_eq!(seeded.assertions.len(), 2);
    let defect = &seeded.assertions[1];
    assert_eq!(defect.status, QaAssertionStatus::Failed);
    assert!(defect.observed.contains("destination /wrong"));
    assert_eq!(seeded.screenshots.len(), 1);
    assert_eq!(defect.evidence_refs, vec![seeded.screenshots[0].id.clone()]);
    assert_eq!(guest.launch_count, 1);
    assert_eq!(guest.action_count, 1);
    assert_eq!(guest.cleanup_count, 1);

    // A rebuilt approved task app gets a fresh QA execution; its corrected
    // destination must pass the same accepted scenario and retain evidence.
    guest.wrong_destination = false;
    let corrected = run_scenario(&request, scenario, &target, &mut guest, &authorization);
    assert!(corrected.completed);
    assert!(
        corrected
            .assertions
            .iter()
            .all(|assertion| { assertion.status == QaAssertionStatus::Passed })
    );
    assert_eq!(guest.launch_count, 2);
    assert_eq!(guest.action_count, 2);
    assert_eq!(guest.cleanup_count, 2);
}

#[derive(Default)]
struct SeededDesktopGuest {
    launched: bool,
    wrong_destination: bool,
    launch_count: usize,
    action_count: usize,
    cleanup_count: usize,
}

impl ComputerQaGuestSessionV1 for SeededDesktopGuest {
    fn begin_scenario(
        &mut self,
        target: &QaComputerTargetV1,
        _scenario_id: &str,
        _deadline: Instant,
    ) -> Result<ComputerQaDisplayV1, String> {
        Ok(ComputerQaDisplayV1 {
            width: target.display_width,
            height: target.display_height,
            dpi: target.display_dpi,
        })
    }

    fn launch_staged_application(
        &mut self,
        _target: &QaComputerTargetV1,
        _deadline: Instant,
    ) -> Result<(), String> {
        self.launched = true;
        self.launch_count += 1;
        Ok(())
    }

    fn observe_state(&mut self, _deadline: Instant) -> Result<String, String> {
        if !self.launched {
            return Ok("guest ready".into());
        }
        Ok(if self.wrong_destination {
            "approved app visible; destination /wrong".into()
        } else {
            "approved app visible; destination /expected".into()
        })
    }

    fn perform_action(
        &mut self,
        _action: &QaDriverActionV1,
        _deadline: Instant,
    ) -> Result<(), String> {
        self.action_count += 1;
        Ok(())
    }

    fn screenshot_png(&mut self, _deadline: Instant) -> Result<Vec<u8>, String> {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            16,
            12,
            image::Rgba([20, 40, 80, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
        Ok(png.into_inner())
    }

    fn end_scenario(&mut self) -> Result<(), String> {
        self.launched = false;
        self.cleanup_count += 1;
        Ok(())
    }
}

fn run_scenario(
    request: &QaRunRequestV1,
    scenario: &QaScenarioV1,
    target: &QaComputerTargetV1,
    guest: &mut SeededDesktopGuest,
    authorization: &QaTargetAuthorization<'_>,
) -> QaDriverOutputV1 {
    let mut driver = ComputerQaScenarioDriverV1 {
        request,
        target,
        guest,
    };
    driver
        .run_scenario(request, scenario, authorization)
        .unwrap()
}
