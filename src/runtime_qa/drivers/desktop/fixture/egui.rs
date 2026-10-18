use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

use sha2::{Digest, Sha256};

use crate::runtime_qa::QaDriverActionV1;

use super::super::{actions, display::DesktopDisplay};

// Production boundary: launch an actual eframe/egui window in the private
// Xvfb display and drive it with the same X11 input path used by desktop QA.
// A unit test cannot prove eframe creates an owned native window or that real
// pointer and keyboard events update its rendered state. External dependency:
// pinned desktop image with rootless Docker in CI. Setup cost: one extra short
// window launch after the candidate image build.
pub(super) fn exercise() -> Result<serde_json::Value, String> {
    let display = DesktopDisplay::start()?;
    let mut process = EguiProcess::start()?;
    let owners = super::super::process::descendants(process.0.id());
    let window = super::wait_for_title(&display, "Egui:Ready", Duration::from_secs(10), &owners)?;
    let before = display.screenshot_png()?;
    let window_id = window.id.to_string();
    actions::execute(
        &display,
        &super::step(QaDriverActionV1::Click {
            window_id: window_id.clone(),
            x: 100,
            y: 36,
        }),
        Duration::from_secs(2),
        &owners,
    )?;
    super::wait_for_title(&display, "Egui:Clicked", Duration::from_secs(5), &owners)?;
    let after_click = display.screenshot_png()?;
    actions::execute(
        &display,
        &super::step(QaDriverActionV1::FocusWindow {
            window_id: window_id.clone(),
        }),
        Duration::from_secs(2),
        &owners,
    )?;
    actions::execute(
        &display,
        &super::step(QaDriverActionV1::Click {
            window_id,
            x: 100,
            y: 88,
        }),
        Duration::from_secs(2),
        &owners,
    )?;
    super::wait_for_title(&display, "Egui:Focused", Duration::from_secs(5), &owners)?;
    actions::execute(
        &display,
        &super::step(QaDriverActionV1::TypeText { text: "ok".into() }),
        Duration::from_secs(2),
        &owners,
    )?;
    super::wait_for_title(&display, "Egui:Typed:ok", Duration::from_secs(5), &owners)?;
    let after_keyboard = display.screenshot_png()?;
    process.stop();

    let hashes = [&before, &after_click, &after_keyboard]
        .map(|image| format!("{:x}", Sha256::digest(image)));
    if hashes[0] == hashes[1] || hashes[1] == hashes[2] {
        return Err("egui fixture screenshots did not reflect native input".into());
    }
    Ok(serde_json::json!({
        "status": "passed",
        "application": "egui",
        "click_assertion": "passed",
        "keyboard_assertion": "passed",
        "screenshot_hashes_distinct": true,
        "screenshots": [
            super::screenshot("before", &before),
            super::screenshot("after_click", &after_click),
            super::screenshot("after_keyboard", &after_keyboard),
        ],
    }))
}

pub(super) fn run_window() -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([640.0, 360.0])
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "Egui:Ready",
        options,
        Box::new(|_| Ok(Box::new(EguiFixture::default()))),
    )
    .map_err(|error| format!("egui fixture could not start: {error}"))
}

#[derive(Default)]
struct EguiFixture {
    input: String,
    clicked: bool,
    focused: bool,
}

impl eframe::App for EguiFixture {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        let title = if !self.input.is_empty() {
            format!("Egui:Typed:{}", self.input)
        } else if self.focused {
            "Egui:Focused".into()
        } else if self.clicked {
            "Egui:Clicked".into()
        } else {
            "Egui:Ready".into()
        };
        ui.ctx()
            .send_viewport_cmd(eframe::egui::ViewportCommand::Title(title));
        ui.add_space(12.0);
        if ui
            .add_sized([240.0, 48.0], eframe::egui::Button::new("Increment"))
            .clicked()
        {
            self.clicked = true;
        }
        ui.add_space(8.0);
        let text_input = ui.add_sized(
            [240.0, 36.0],
            eframe::egui::TextEdit::singleline(&mut self.input).hint_text("Type here"),
        );
        self.focused |= text_input.gained_focus();
    }
}

struct EguiProcess(Child);

impl EguiProcess {
    fn start() -> Result<Self, String> {
        Command::new(
            std::env::current_exe()
                .map_err(|error| format!("could not locate desktop QA driver: {error}"))?,
        )
        .arg("--fixture-egui-window")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("DISPLAY", super::super::display::DISPLAY_NAME)
        .env("LANG", "C.UTF-8")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(Self)
        .map_err(|error| format!("could not launch egui fixture: {error}"))
    }

    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for EguiProcess {
    fn drop(&mut self) {
        self.stop();
    }
}
