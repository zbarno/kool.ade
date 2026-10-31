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
            onboarding_launcher: Default::default(),
            onboarding_open: None,
            onboarding_recovery: Default::default(),
            launch_history_jobs: {
                #[cfg(not(test))]
                {
                    let manager =
                        crate::core::managed_workspace::WorkspaceManager::from_state_root();
                    vec![std::thread::spawn(move || {
                        manager
                            .flush_pending_opens()
                            .map_err(|error| error.to_string())
                    })]
                }
                #[cfg(test)]
                {
                    Vec::new()
                }
            },
            onboarding_creation: None,
            task_harness: None,
            pending_feature_generation: None,
            screen: Screen::Welcome,
            spawn_target_override: None,
            dialog: None,
            toasts,
            conn_error: None,
            session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            last_git_refresh: Instant::now(),
            display_refresh: None,
            clone_job: None,
            planning_sync_job: None,
            conn_workspace_id: None,
            welcome_draft: String::new(),
            wiz: None,
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
