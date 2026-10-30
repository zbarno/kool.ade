use super::*;

impl Default for KooladeApp {
    fn default() -> Self {
        let toasts = ToastQueue::default();
        // Bootstrap must also run on the Welcome screen, before a workspace or
        // Docker daemon exists. Tests opt in explicitly to avoid probing the
        // developer machine while constructing UI fixtures.
        #[cfg(not(test))]
        let setup_probe_cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )));
        #[cfg(test)]
        let setup_probe_cancel = None;
        #[cfg(not(test))]
        let setup_probe = {
            let cancellation = setup_probe_cancel.as_ref().unwrap().clone();
            Some(std::thread::spawn(move || {
                setup_attention::ProbeOutput::Scanned(crate::app::setup_attention::run_scan(
                    true,
                    cancellation,
                    None,
                ))
            }))
        };
        #[cfg(test)]
        let setup_probe = None;
        #[cfg(test)]
        let setup_state_root =
            std::env::temp_dir().join(format!("koolade-app-setup-state-{}", uuid::Uuid::new_v4()));
        #[cfg(test)]
        let setup_task_state_path = setup_state_root.join("machine-setup").join("tasks.json");

        Self {
            onboarding: welcome::onboarding::Onboarding::load({
                #[cfg(test)]
                {
                    setup_state_root.join("onboarding.json")
                }
                #[cfg(not(test))]
                {
                    crate::persistence::state_root().join("onboarding.json")
                }
            }),
            onboarding_creation: None,
            onboarding_inspection: None,
            onboarding_project_kind: None,
            task_harness: None,
            pending_feature_generation: None,
            screen: Screen::Welcome,
            spawn_target_override: None,
            dialog: None,
            toasts,
            conn_path: std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            conn_create_title: String::new(),
            conn_github: String::new(),
            conn_error: None,
            session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            last_git_refresh: Instant::now(),
            display_refresh: None,
            clone_job: None,
            planning_sync_job: None,
            conn_join_remote: String::new(),
            conn_workspace_id: None,
            clone_computation_override: None,
            cached_user: CurrentUser::new("", Vec::new()),
            attention: Default::default(),
            setup_attention: Vec::new(),
            machine_setup_tasks: Vec::new(),
            setup_attention_notified: None,
            setup_probe,
            setup_probe_cancel,
            setup_commit_lock: Default::default(),
            #[cfg(test)]
            setup_task_state_path,
            setup_retry_requested: false,
            setup_refresh_pending: None,
            setup_image_preparation: crate::app::bootstrap_image_prepare::Controller::default(),
            claude_install: crate::app::bootstrap_remediation::ClaudeInstallController::new(),
            setup_remediation: crate::app::bootstrap_remediation::Controller::default(),
            bootstrap_conversation: crate::app::root::bootstrap_conversation::Controller::default(),
            #[cfg(test)]
            attention_fixture: Default::default(),
            #[cfg(test)]
            _test_temp_roots: vec![TestTempRoot(setup_state_root)],
            synth: Vec::new(),
        }
    }
}

#[cfg(test)]
impl KooladeApp {
    pub(crate) fn set_setup_task_state_path(&mut self, path: std::path::PathBuf) {
        self.setup_task_state_path = path;
    }
}
