use super::*;

pub(super) fn init_git(root: &std::path::Path) {
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
}

pub(super) struct ReplyHarness {
    pub(super) prompts: Arc<Mutex<Vec<String>>>,
    pub(super) reply: String,
}

pub(super) struct SynchronizedReplyHarness {
    pub(super) prompts: Arc<Mutex<Vec<String>>>,
    pub(super) rendezvous: Arc<(Mutex<usize>, std::sync::Condvar)>,
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

impl crate::harness::AiHarness for SynchronizedReplyHarness {
    fn label(&self) -> String {
        "synchronized task conversation fixture".into()
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
        let (arrived, changed) = &*self.rendezvous;
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut count = arrived.lock().unwrap();
        *count += 1;
        changed.notify_all();
        while *count < 2 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(crate::error::AppError::Other(
                    "second synchronized task turn did not start".into(),
                ));
            }
            let (next, timeout) = changed.wait_timeout(count, remaining).unwrap();
            count = next;
            if timeout.timed_out() && *count < 2 {
                return Err(crate::error::AppError::Other(
                    "second synchronized task turn did not start".into(),
                ));
            }
        }
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
        // The full UI suite runs independent turns in parallel on shared CI hosts.
        assert!(
            started.elapsed() < Duration::from_secs(30),
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
    let _ = app.adopt_turn(&mut project, outcome, key.as_deref(), None, false);
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
    let mut identities = p
        .task_documents
        .iter()
        .filter_map(|doc| {
            doc.identity
                .as_ref()
                .map(|identity| (doc.path.clone(), identity.uid.clone()))
        })
        .collect::<Vec<_>>();
    identities.extend(p.state.active_features.iter().filter_map(|(id, markdown)| {
        let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .ok()
            .flatten()?;
        Some((format!("feature:{id}"), identity.uid))
    }));
    identities.extend(
        p.state
            .items
            .iter()
            .chain(&p.state.resolved_items)
            .filter_map(|item| {
                item.uid
                    .as_ref()
                    .map(|uid| (item.conversation_key().to_owned(), uid.clone()))
            }),
    );
    let legacy_migration_keys = p
        .task_documents
        .iter()
        .filter(|doc| doc.identity.is_some())
        .map(|doc| doc.path.clone())
        .collect::<Vec<_>>();
    store
        .bind_conversation_identities(&identities, &legacy_migration_keys)
        .expect("reloaded task histories use the board's stable identities");
    store.ensure_loaded(&p.chat_slug);
    store.messages["CLR-001"].len()
}
