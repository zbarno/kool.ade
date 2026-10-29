use crate::runtime_qa::QaDriverActionV1;

/// Exercises actions that were previously covered only by driver unit tests.
/// The fixture changes its window title only after receiving each X11 event.
pub(crate) fn happy_steps() -> Vec<(QaDriverActionV1, Option<&'static str>, bool)> {
    vec![
        (QaDriverActionV1::LaunchValidatedTarget, None, false),
        (
            QaDriverActionV1::WaitForWindow,
            Some("koolade nonrust qa: ready"),
            true,
        ),
        (
            QaDriverActionV1::ListWindows,
            Some("koolade nonrust qa"),
            false,
        ),
        (
            QaDriverActionV1::AssertRegionVisible {
                window_id: "active".into(),
                x: 80,
                y: 20,
                width: 60,
                height: 32,
            },
            Some("region visible"),
            false,
        ),
        (
            QaDriverActionV1::Click {
                window_id: "active".into(),
                x: 100,
                y: 36,
            },
            Some("clicked"),
            false,
        ),
        (
            QaDriverActionV1::WaitForWindow,
            Some("koolade nonrust qa: clicked"),
            false,
        ),
        (
            QaDriverActionV1::Click {
                window_id: "active".into(),
                x: 100,
                y: 88,
            },
            Some("clicked"),
            false,
        ),
        (
            QaDriverActionV1::TypeText { text: "ok".into() },
            Some("typed 2 characters"),
            false,
        ),
        (
            QaDriverActionV1::WaitForWindow,
            Some("koolade nonrust qa: typed:ok"),
            true,
        ),
        (
            QaDriverActionV1::ResizeWindow {
                window_id: "active".into(),
                width: 640,
                height: 400,
            },
            Some("resized"),
            false,
        ),
        (
            QaDriverActionV1::DoubleClick {
                window_id: "active".into(),
                x: 220,
                y: 36,
            },
            Some("double-click"),
            false,
        ),
        (
            QaDriverActionV1::WaitForWindow,
            Some("koolade nonrust qa: doubleclick"),
            false,
        ),
        (
            QaDriverActionV1::Drag {
                window_id: "active".into(),
                start_x: 180,
                start_y: 180,
                end_x: 260,
                end_y: 180,
            },
            Some("dragged"),
            false,
        ),
        (
            QaDriverActionV1::WaitForWindow,
            Some("koolade nonrust qa: dragged"),
            false,
        ),
        (
            QaDriverActionV1::FocusWindow {
                window_id: "active".into(),
            },
            Some("focused window"),
            false,
        ),
        (
            QaDriverActionV1::KeyChord {
                keys: vec!["ctrl".into(), "a".into()],
            },
            Some("key chord"),
            false,
        ),
        (
            QaDriverActionV1::WaitForWindow,
            Some("koolade nonrust qa: keychord"),
            false,
        ),
        (
            QaDriverActionV1::Scroll {
                window_id: "active".into(),
                delta_x: 0,
                delta_y: -2,
            },
            Some("scrolled"),
            false,
        ),
        (
            QaDriverActionV1::WaitForWindow,
            Some("koolade nonrust qa: scrolled"),
            false,
        ),
        (
            QaDriverActionV1::ReadAppLogs,
            Some("application logs requested"),
            false,
        ),
        (
            QaDriverActionV1::CloseWindow {
                window_id: "active".into(),
            },
            Some("closed"),
            false,
        ),
    ]
}
