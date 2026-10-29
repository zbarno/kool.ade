use super::Fixture;
use crate::project_runtime::{AppSurface, TargetCapabilityState, discover_manifest};
use std::path::Path;

#[test]
fn mobile_and_device_fixtures_are_unsupported_without_visual_qa_applicability() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_qa");
    let cases = [
        ("flutter_android", "Dart", "pubspec.yaml"),
        ("flutter_ios", "Dart", "pubspec.yaml"),
        ("embedded_device", "C/C++", "CMakeLists.txt"),
    ];
    let available_tools = std::collections::BTreeSet::new();
    let available_dependencies = std::collections::BTreeSet::new();
    // Even with both existing visual drivers available, unsupported targets
    // must not be assigned a browser or Linux desktop QA claim.
    let available_drivers = ["web-driver".into(), "linux-desktop-driver".into()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();

    for (fixture, language, evidence_path) in cases {
        let context = Fixture::context();
        let root = fixture_root.join(fixture);
        let manifest = discover_manifest(&root, &context).unwrap();
        assert_eq!(manifest.detected_languages, [language], "{fixture}");
        assert_eq!(manifest.evidence_refs, [evidence_path], "{fixture}");
        assert_eq!(manifest.candidate_targets.len(), 1, "{fixture}");
        let target = &manifest.candidate_targets[0];
        assert_eq!(
            target.app_surface,
            AppSurface::UnsupportedTarget,
            "{fixture}"
        );
        assert_eq!(
            target.capability_state,
            TargetCapabilityState::UnsupportedPlatform,
            "{fixture}"
        );
        assert!(target.supported_driver_ids.is_empty(), "{fixture}");
        assert!(target.readiness_probe_ref.is_none(), "{fixture}");
        assert!(manifest.recipes.is_empty(), "{fixture}");
        assert!(manifest.user_overrides.is_empty(), "{fixture}");

        let inventory = crate::project_runtime_capability::ApprovedCapabilityInventory {
            manifest: &manifest,
            available_tool_ids: &available_tools,
            available_dependency_ids: &available_dependencies,
            available_driver_ids: &available_drivers,
        };
        let assessment = crate::project_runtime_capability::assess_manifest_target(
            &manifest,
            &target.target_id,
            &context.owning_task_id,
            &inventory,
        )
        .unwrap();
        assert_eq!(
            assessment.capability_state,
            TargetCapabilityState::UnsupportedPlatform,
            "{fixture}"
        );
        assert_eq!(
            assessment.report.qa_applicability,
            crate::project_runtime_capability::QaApplicability::Unsupported,
            "{fixture}"
        );
        assert_eq!(assessment.report.selected_driver_id, None, "{fixture}");
    }
}
