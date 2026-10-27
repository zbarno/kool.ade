use super::{QaDriverActionV1, QaScenarioStepV1};

pub(super) fn origin_escape_cases() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        (
            "metadata redirect",
            "http://qa-service/redirect",
            "redirect escaped preview origin: http://169.254.169.254",
        ),
        (
            "metadata fetch",
            "http://qa-service/?defect=metadata",
            "request escaped preview origin: http://169.254.169.254",
        ),
        (
            "metadata iframe",
            "http://qa-service/?defect=iframe",
            "request escaped preview origin: http://169.254.169.254",
        ),
        (
            "external WebSocket",
            "http://qa-service/?defect=websocket",
            "websocket escaped preview origin: ws://example.com",
        ),
        (
            "localhost request",
            "http://qa-service/?defect=localhost",
            "request escaped preview origin: http://127.0.0.1:4173/",
        ),
        (
            "alternate preview port",
            "http://qa-service/?defect=alternate-port",
            "request escaped preview origin: http://qa-preview:8081/",
        ),
        (
            "raw IPv4 portal address",
            "http://qa-service/?defect=ip-port",
            "request escaped preview origin: http://127.0.0.1:8080/",
        ),
        (
            "raw IPv6 portal address",
            "http://qa-service/?defect=ipv6-port",
            "request escaped preview origin: http://[::1]:8080/",
        ),
        (
            "adjacent container access",
            "http://qa-service/?defect=adjacent",
            "request escaped preview origin: http://qa-task:4173/__sink_hit",
        ),
        (
            "DNS-rebound internal host",
            "http://qa-service/?defect=dns-rebind",
            "request escaped preview origin: http://rebound.internal:4173/__sink_hit",
        ),
        (
            "loopback websocket portal",
            "http://qa-service/?defect=ws-loopback",
            "websocket escaped preview origin: ws://127.0.0.1:8080/",
        ),
        (
            "external internet egress",
            "http://qa-service/?defect=internet",
            "request escaped preview origin: http://www.example.invalid/",
        ),
    ]
}

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
