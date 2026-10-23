use super::*;

#[test]
fn request_and_scenario_are_bounded_and_cover_requirements() {
    let request = request();
    let scenario = scenario(&request);
    assert!(validate_scenario_set(&request, std::slice::from_ref(&scenario)).is_ok());

    let mut bad = scenario;
    bad.steps[0].driver_action = QaDriverActionV1::NavigateAllowedUrl {
        url: "https://outside.invalid/".into(),
    };
    assert!(bad.validate(&request).is_err());
}
