use super::*;

impl KooladeApp {
    pub(super) fn start_turn(&mut self, text: &str) {
        self.start_turn_with_purpose(text, crate::core::workflow::TurnPurpose::Interview);
    }

    pub(super) fn start_turn_with_purpose(
        &mut self,
        text: &str,
        purpose: crate::core::workflow::TurnPurpose,
    ) {
        self.start_turn_for_feature(text, purpose, None);
    }

    pub(super) fn start_comparison_turn(&mut self, feature_id: &str) {
        let request = format!("Compare plans for feature {feature_id}.");
        self.start_turn_for_feature(
            &request,
            crate::core::workflow::TurnPurpose::ComparePlans,
            Some(feature_id),
        );
    }

    pub(super) fn start_turn_for_feature(
        &mut self,
        text: &str,
        purpose: crate::core::workflow::TurnPurpose,
        comparison_feature: Option<&str>,
    ) {
        self.start_turn_for_work(text, purpose, comparison_feature, None);
    }

    pub(super) fn start_turn_for_work(
        &mut self,
        text: &str,
        purpose: crate::core::workflow::TurnPurpose,
        comparison_feature: Option<&str>,
        work_key: Option<String>,
    ) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some()
            || (!project.active_implementations.is_empty()
                && purpose == crate::core::workflow::TurnPurpose::GenerateTasks)
        {
            return;
        }
        project.activity.manager = None;
        // A new planning turn belongs to the main planning flow. A task chat
        // selected earlier must not capture its progress or completion.
        project.task_chats.active = None;
        project.bind_task_conversation_identities();
        let task_context = project.task_interaction_context(text);
        // Main Chat is retained as history only; planning turns are grounded
        // in board state and task context, never its transcript.
        let recent = Vec::new();
        let key = work_key.unwrap_or_else(|| {
            format!(
                "planning:{}",
                ChatMessage::new(ChatRole::User, text, None).id
            )
        });
        let existing = project.planning_work.iter().any(|work| work.key == key);
        if !existing {
            project
                .planning_work
                .push(crate::core::planning_work::Work::new(
                    key.clone(),
                    format!("Plan {}", crate::core::context_build::clip(text, 100)),
                    text.into(),
                    "Planning in progress".into(),
                ));
            if let Err(error) =
                crate::core::planning_work::save(&project.state.repo_root, &project.planning_work)
            {
                project.planning_work.pop();
                self.toasts
                    .danger(format!("Cannot record planning work: {error}"));
                return;
            }
        }
        let work = project
            .planning_work
            .iter()
            .find(|work| work.key == key)
            .cloned();
        let work_guidance = work
            .as_ref()
            .map(|work| format!("{} — {}", work.kind.label(), work.kind.planning_guidance()))
            .unwrap_or_else(|| "Feature".into());
        let activity = match purpose {
            crate::core::workflow::TurnPurpose::GenerateTasks => "Generating implementation tasks…",
            crate::core::workflow::TurnPurpose::ReviewForGeneration => {
                "Reviewing the approved specification…"
            }
            _ if work
                .as_ref()
                .is_some_and(|work| work.kind == crate::core::planning_work::WorkKind::Bug) =>
            {
                "Building context for bug triage…"
            }
            _ => "Building project context…",
        };
        project.active_planning_work = Some(key);
        let inputs = crate::core::turn::TurnInputs {
            state: project.state.clone(),
            user_message: format!(
                "Task kind and guidance: {work_guidance}. User request: {text}\n\n{task_context}\n\n[Application project context: active task worker={:?}; auto queue running={}; task count={}; recent task states={:?}. Continue managing the project and engaging this user while the isolated worker handles implementation. Do not claim to steer or stop a worker through prose; task controls manage that. Planning answers may update the specification normally.]",
                project.active_implementations.keys().collect::<Vec<_>>(),
                project.queue.running,
                project.implementation_states.len(),
                project
                    .implementation_states
                    .iter()
                    .rev()
                    .take(5)
                    .map(|(ticket, state)| (ticket, state.status.label()))
                    .collect::<Vec<_>>()
            ),
            recent_chat: recent,
            purpose,
            comparison_feature: comparison_feature.map(str::to_owned),
        };
        let harness = configured_harness(&mut self.task_harness);
        let ctrl = TurnController::start(inputs, harness);
        project.active_turn = Some(std::rc::Rc::new(ctrl));
        project.live_progress = crate::harness::LiveProgress {
            activity: Some(activity.into()),
            ..Default::default()
        };
    }

    pub(super) fn disconnect(&mut self) {
        self.pending_feature_generation = None;
        if let Screen::Connected(p) = &mut self.screen {
            for ctrl in p.active_implementations.values() {
                ctrl.request_cancel();
            }
            if p.active_turn.is_some() {
                if let Some(ctrl) = &p.active_turn {
                    ctrl.request_cancel();
                }
                self.toasts
                    .warning(format!("Turn aborted; disconnected from {}", p.state.title));
            }
        }
        self.dialog = None;
        self.synth.clear();
        self.screen = Screen::Welcome;
    }

    /// Workspace menu 'Open workspace': spawn a DETACHED sibling Koolade
    /// process that boots to the initial (Welcome) screen.
    ///
    /// Deliberate contract contrast with [`Self::disconnect`]: this hands
    /// out a second window and touches NONE of this session — no
    /// `request_cancel` on the active turn or implementations, no
    /// `screen`/`dialog`/`synth`/`queue` mutation. Spawn + toast only.
    pub fn open_workspace(&mut self) {
        let target: Result<std::path::PathBuf, String> = self
            .spawn_target_override
            .clone()
            .map(Ok)
            .unwrap_or_else(crate::app::spawn::resolve_self_executable);
        match target {
            Ok(bin) => match crate::app::spawn::spawn_sibling(&bin) {
                Ok(()) => self.toasts.info("Opening a new Kool.ad/e window"),
                Err(message) => self.toasts.warning(message),
            },
            Err(message) => self.toasts.warning(message),
        }
    }

    pub(super) fn copy_spec_to_clipboard(&mut self) {
        let Screen::Connected(p) = &self.screen else {
            return;
        };
        let text = p
            .live_progress
            .specification
            .as_deref()
            .or(p.state.spec_text.as_deref())
            .unwrap_or_default()
            .to_owned();
        clipboard_put(&text);
        self.toasts.info(format!(
            "Copied {} characters to clipboard",
            text.chars().count()
        ));
    }
}
