//! Opt-in screenshot review through the production painters and click paths.
//! Run with isolated KOOLADE_HOME and KOOLADE_UI_REVIEW_DIR; no workers are ticked.
use super::*;
#[path = "visual_gallery/fixtures.rs"]
mod fixtures;
#[path = "visual_gallery/renderer.rs"]
mod renderer;

#[test]
#[ignore = "GPU screenshot review; requires isolated KOOLADE_HOME and KOOLADE_UI_REVIEW_DIR"]
fn render_workspace_review_gallery() {
    let dir = std::path::PathBuf::from(
        std::env::var("KOOLADE_UI_REVIEW_DIR").expect("review output directory"),
    );
    assert!(
        std::env::var_os("KOOLADE_HOME").is_some(),
        "use isolated app storage"
    );
    std::fs::create_dir_all(&dir).unwrap();
    for size in [
        [1600, 900],
        [1280, 720],
        [900, 720],
        [360, 720],
        [360, 480],
        [1280, 480],
    ] {
        let mut gallery = Gallery::new(size, dir.clone());
        gallery.save("board");
        gallery.click("First task");
        gallery.save("task-details");
        gallery.scroll_to_bottom();
        gallery.save("task-details-scrolled");
        gallery.ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("koolade_task_activity"),
                ".koolade-packet/planning/tasks/fixture/001-task.md".to_owned(),
            )
        });
        gallery.save("all-activity");
        gallery.escape();
        gallery.click("Workspace");
        gallery.save("workspace-menu");
        gallery.click("Settings…");
        fixtures::settings(&gallery.ctx);
        gallery.save("settings-general");
        for label in [
            "Appearance",
            "Coding Tools",
            "Models & Routing",
            "Automation",
            "Project & Git",
            "People & Stakeholders",
        ] {
            gallery.ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("koolade_settings_open_page"),
                    label.to_owned(),
                )
            });
            gallery.save(&format!(
                "settings-{}",
                label.split_whitespace().next().unwrap().to_lowercase()
            ));
        }
        gallery.escape();
        gallery.click("+ New Task");
        gallery.save("new-task");
        gallery.escape();
        gallery.click("Specification");
        gallery.save("specification");
        gallery
            .ctx
            .data_mut(|data| data.insert_temp(egui::Id::new("koolade_document_tab"), true));
        fixtures::planning(&mut gallery.app);
        gallery.save("board-planning");
        for (key, name) in [
            ("CLR-010", "question-details"),
            ("planning-review", "planning-details"),
        ] {
            gallery.ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_selected_planning"), key.to_owned())
            });
            gallery.save(name);
            gallery.scroll_to_bottom();
            gallery.save(&format!("{name}-scrolled"));
            gallery.escape();
        }
        fixtures::feature(&mut gallery.app);
        gallery.ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("koolade_selected_planning"),
                "feature-approval:CHG-001".to_owned(),
            )
        });
        gallery.save("feature-comparison");
        gallery.scroll_to_bottom();
        gallery.save("feature-comparison-scrolled");
        gallery.escape();
        gallery.app.setup_attention = Some(crate::app::setup_attention::SetupIssue::bubblewrap(
            "Sandbox setup has not been completed.",
        ));
        gallery.ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("koolade_selected_planning"),
                "setup:bubblewrap".to_owned(),
            )
        });
        gallery.save("setup-details");
        gallery.escape();
        gallery.app.setup_attention = None;
        for (label, name) in [("Import references…", "import"), ("MCP servers…", "mcp")] {
            gallery.click("Workspace");
            gallery.click(label);
            gallery.save(name);
            gallery.escape();
        }
        gallery.app.screen = Screen::Welcome;
        gallery.app.conn_path.clear();
        gallery.save("welcome");
        gallery.scroll_to_bottom();
        gallery.save("welcome-scrolled");
        gallery.click("Configure coding tools…");
        fixtures::settings(&gallery.ctx);
        let dialog = gallery
            .ctx
            .data_mut(|data| {
                data.remove_temp::<crate::app::dialogs::DlgHarnessSetup>(egui::Id::new(
                    "koolade_settings_harness_draft",
                ))
            })
            .unwrap();
        gallery.app.dialog = Some(Dialog::HarnessSetup(dialog));
        gallery.save("welcome-coding-tools");
        gallery.scroll_to_bottom();
        gallery.save("welcome-coding-tools-scrolled");
        gallery.escape();
    }
}

struct Gallery {
    app: KooladeApp,
    ctx: egui::Context,
    renderer: renderer::Renderer,
    size: [u32; 2],
    dir: std::path::PathBuf,
    frame: u64,
}

impl Gallery {
    fn new(size: [u32; 2], dir: std::path::PathBuf) -> Self {
        let mut app = fixture();
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.state.title = "Orbit · Team workspace".into();
        project.git.branch = "feat/improve-project-workflow".into();
        let mut active_doc = project.task_documents[0].clone();
        active_doc.path = ".koolade-packet/planning/tasks/fixture/004-task.md".into();
        active_doc.title = "Refine the workspace navigation and onboarding flow".into();
        project.active_implementations.insert(
            active_doc.path.clone(),
            crate::core::implementation::Controller::idle_fixture(),
        );
        project.task_documents.push(active_doc);
        project.state.spec_text = Some("# Orbit workspace\n\nA shared place for teams to turn ideas into a clear implementation plan.\n\n## Project goals\n\n- Capture decisions alongside the work.\n- Make the next step clear for every team member.\n- Review changes before publishing.\n\n## Working together\n\nEach task keeps its conversation, activity, and review history. Invite your team and start with a feature or a question.".into());
        let now = chrono::Utc::now().timestamp_millis() / 10_000;
        for doc in &project.task_documents {
            let progress = project.activity.tasks.entry(doc.path.clone()).or_default();
            // Explicit synthetic telemetry for this review fixture only.
            progress.telemetry.samples = (0..60)
                .map(|i| {
                    (
                        now - 59 + i,
                        if i % 7 < 3 { (i % 11 + 1) as u64 } else { 0 },
                    )
                })
                .collect();
            progress.telemetry.updated_ms = Some(now * 10_000);
            progress.telemetry.started_ms = Some((now - 60) * 10_000);
            progress.telemetry.updates = progress
                .telemetry
                .samples
                .iter()
                .map(|(_, count)| count)
                .sum();
            progress.activity = Some("Reviewing the latest workspace changes".into());
            progress.response = "The workspace navigation is ready for review. The remaining checks cover narrow windows and keyboard navigation.".into();
        }
        Self {
            app,
            ctx: super::mockup_layout::styled_context(),
            renderer: renderer::Renderer::new(),
            size,
            dir,
            frame: 0,
        }
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.frame += 1;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(self.size[0] as f32, self.size[1] as f32),
                )),
                time: Some(self.frame as f64 / 10.0),
                events,
                ..Default::default()
            },
            |ui| {
                self.app.paint_screen(ui);
            },
        );
        self.renderer.update(&output);
        output.textures_delta.clear();
        output
    }

    fn save(&mut self, name: &str) {
        self.frame(vec![egui::Event::PointerMoved(egui::pos2(1.0, 1.0))]);
        for _ in 0..4 {
            self.frame(vec![]);
        }
        let output = self.frame(vec![]);
        self.renderer.save(
            &self.ctx,
            output,
            self.size,
            &self
                .dir
                .join(format!("{}x{}-{name}.png", self.size[0], self.size[1])),
        );
    }

    fn click(&mut self, text: &str) {
        let output = self.frame(vec![]);
        let pos = text_position(&output, text).unwrap_or_else(|| panic!("missing {text}"));
        for pressed in [true, false] {
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
        }
        self.frame(vec![egui::Event::PointerGone]);
    }

    fn scroll_to_bottom(&mut self) {
        for _ in 0..6 {
            self.frame(vec![
                egui::Event::PointerMoved(egui::pos2(
                    self.size[0] as f32 * 0.65,
                    self.size[1] as f32 * 0.6,
                )),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -600.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ]);
        }
    }

    fn escape(&mut self) {
        self.frame(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]);
        self.frame(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Default::default(),
        }]);
        self.frame(vec![]);
    }
}
