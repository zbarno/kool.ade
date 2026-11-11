use super::*;

#[test]
fn agent_handback_allows_only_a_fresh_screenshot_before_more_actions() {
    use crate::computer_use::{
        ComputerActionActorV1 as Actor, ComputerActionPayloadV1 as Payload,
        mcp::ComputerUseMcpRequestV1 as Request,
    };

    let session_id = "00000000-0000-4000-8000-000000000001".to_owned();
    let screenshot = Request::Observe {
        session_id: session_id.clone(),
        observation: ComputerUseObservationKindV1::Screenshot,
    };
    let windows = Request::Observe {
        session_id: session_id.clone(),
        observation: ComputerUseObservationKindV1::Windows,
    };
    let input = Request::Action {
        session_id,
        payload: Payload::TypeText {
            text: "synthetic fixture only".into(),
            window_id: "synthetic-window".into(),
            observation_revision: 1,
        },
        timeout_ms: 1_000,
        idempotency_key: None,
    };

    assert!(!handback_requires_fresh_screenshot(
        Actor::Agent,
        true,
        &screenshot
    ));
    assert!(handback_requires_fresh_screenshot(
        Actor::Agent,
        true,
        &windows
    ));
    assert!(handback_requires_fresh_screenshot(
        Actor::Agent,
        true,
        &input
    ));
    assert!(!handback_requires_fresh_screenshot(
        Actor::Agent,
        false,
        &input
    ));
    assert!(!handback_requires_fresh_screenshot(
        Actor::Human,
        true,
        &input
    ));
}
