pub(super) fn valid_action_command(command: &str) -> bool {
    matches!(
        command,
        "api_request"
            | "launch_validated_target"
            | "wait_for_window"
            | "list_windows"
            | "focus_window"
            | "resize_window"
            | "click"
            | "double_click"
            | "drag"
            | "type_text"
            | "key_chord"
            | "scroll"
            | "wait_for_visible"
            | "navigate_allowed_url"
            | "find_by_role"
            | "find_by_test_id"
            | "fill"
            | "press"
            | "select"
            | "screenshot"
            | "read_app_logs"
            | "collect_console"
            | "collect_network_failures"
            | "open"
            | "wait_for"
            | "evaluate_limited_assertions"
            | "set_viewport"
            | "close"
    )
}

pub(super) fn requires_state_transition(command: &str) -> bool {
    matches!(
        command,
        "api_request"
            | "click"
            | "double_click"
            | "drag"
            | "type_text"
            | "key_chord"
            | "fill"
            | "press"
            | "select"
    )
}
