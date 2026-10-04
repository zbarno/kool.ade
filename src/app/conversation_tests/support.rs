use super::*;

pub(super) struct ReplyHarness {
    pub(super) prompts: Arc<Mutex<Vec<String>>>,
    pub(super) reply: String,
}

pub(super) struct StoppedHarness {
    pub(super) wait_for_cancel: bool,
}

impl crate::harness::AiHarness for StoppedHarness {
    fn label(&self) -> String {
        "stopped fixture".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        if self.wait_for_cancel {
            while !request.cancel.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Err(crate::error::AppError::Other(
            if self.wait_for_cancel {
                "cancelled"
            } else {
                "provider unavailable"
            }
            .into(),
        ))
    }
}

impl crate::harness::AiHarness for ReplyHarness {
    fn label(&self) -> String {
        "task conversation fixture".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.prompts
            .lock()
            .unwrap()
            .push(request.prompt_body.clone());
        Ok(crate::harness::HarnessOutcome {
            final_text: self.reply.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

pub(super) fn complete(app: &mut KooladeApp) {
    let mut project = match std::mem::replace(&mut app.screen, Screen::Welcome) {
        Screen::Connected(project) => project,
        _ => panic!("not connected"),
    };
    let started = Instant::now();
    let key = project.task_turns.keys().next().cloned();
    let outcome = loop {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "focused turn did not finish"
        );
        if let Some(TurnEvt::Done(outcome)) = key
            .as_ref()
            .and_then(|key| project.task_turns.get(key))
            .or(project.active_turn.as_ref())
            .expect("reply must start a turn")
            .poll(Duration::from_millis(20))
        {
            break *outcome;
        }
    };
    if let Some(key) = &key {
        project.task_turns.remove(key);
        project.task_live.remove(key);
    }
    project.task_chats.active = key.clone();
    let main = if key.is_some() {
        project.active_turn.take()
    } else {
        None
    };
    let live = std::mem::take(&mut project.live_progress);
    let _ = app.adopt_turn(&mut project, outcome);
    project.active_turn = main;
    project.live_progress = live;
    app.screen = Screen::Connected(project);
}

pub(super) fn click_last(app: &mut KooladeApp, ctx: &egui::Context, label: &str) {
    let output = frame(app, ctx, vec![]);
    let pos = output
        .shapes
        .iter()
        .rev()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == label
            {
                return Some(text.pos + text.galley.mesh_bounds.center().to_vec2());
            }
            None
        })
        .unwrap_or_else(|| panic!("missing {label}"));
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}

pub(super) fn persisted_len_helper(
    app: &KooladeApp,
    mut store: crate::persistence::task_chats::TaskChats,
) -> usize {
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    store.ensure_loaded(&p.chat_slug);
    store.messages["CLR-001"].len()
}
