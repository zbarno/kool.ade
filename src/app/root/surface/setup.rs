macro_rules! setup_surface_methods {
    () => {
        fn setup_image_preparation(&mut self, ui: &mut egui::Ui, prerequisite_id: &str) {
            let (completed, request) = self.setup_image_preparation.paint(ui, prerequisite_id);
            self.publish_setup_task_store_update();
            if completed {
                self.refresh_setup_attention(true);
            }
            if let Some(task_id) = request {
                self.dispatch_ui_command(crate::ui::ApplicationCommand::ApproveSetupTaskFix {
                    task_id,
                });
            }
        }

        fn supports_provider_install(&self, task_id: &str) -> bool {
            task_id == "setup:cli:claude:install"
                && crate::app::bootstrap_remediation::ClaudeInstallController::supports_current_host()
        }

        fn setup_provider_install(&mut self, ui: &mut egui::Ui, task_id: &str) {
            if task_id != "setup:cli:claude:install" {
                return;
            }
            ui.add_space(crate::ui::theme::spacing::M);
            ui.label(crate::ui::theme::section_heading("Install Claude Code"));
            ui.label(
                "Review the exact Anthropic release, signature, checksum, and current-user file changes before anything is downloaded. Sign-in and model access remain separate provider steps.",
            );
            let request = self
                .claude_install
                .preview()
                .and_then(|preview| preview.show_confirmation(ui));
            let install_status = self.claude_install.status().clone();
            match install_status {
                crate::app::bootstrap_remediation::ClaudeInstallStatus::Running => {
                    let phase = match self.claude_install.progress() {
                        Some(crate::app::bootstrap_remediation::ClaudeInstallProgress::Download) => {
                            "Downloading the pinned release"
                        }
                        Some(
                            crate::app::bootstrap_remediation::ClaudeInstallProgress::TrustVerification,
                        ) => "Verifying Anthropic's signature and checksum",
                        Some(
                            crate::app::bootstrap_remediation::ClaudeInstallProgress::CurrentUserInstall,
                        ) => "Installing for the current user",
                        Some(
                            crate::app::bootstrap_remediation::ClaudeInstallProgress::FreshProviderProbe,
                        ) => "Checking the installed Claude Code CLI",
                        None => "Preparing the approved install",
                    };
                    ui.label(phase);
                    if ui.button("Cancel install").clicked() {
                        if let Err(error) = self.claude_install.cancel_review() {
                            self.toasts.warning(format!(
                                "Could not cancel the provider install: {error}"
                            ));
                        }
                        self.publish_setup_task_store_update();
                    }
                }
                crate::app::bootstrap_remediation::ClaudeInstallStatus::Finished(Ok(())) => {
                    ui.label("The install completed and the CLI passed its fresh provider probe.");
                }
                crate::app::bootstrap_remediation::ClaudeInstallStatus::Finished(Err(_)) => {
                    ui.label("The install did not complete. Review the setup task and try again.");
                }
                crate::app::bootstrap_remediation::ClaudeInstallStatus::Idle
                | crate::app::bootstrap_remediation::ClaudeInstallStatus::NeedsApproval => {}
            }
            if self.claude_install.preview().is_some() && ui.button("Cancel review").clicked() {
                if let Err(error) = self.claude_install.cancel_review() {
                    self.toasts.warning(format!(
                        "Could not cancel the provider install review: {error}"
                    ));
                }
                self.publish_setup_task_store_update();
                return;
            }
            if let Some(request) = request {
                match self.claude_install.approve(request) {
                    Ok(_) => {
                        self.publish_setup_task_store_update();
                        self.toasts
                            .info("The approved Claude Code install is running for this user.");
                    }
                    Err(error) => self
                        .toasts
                        .warning(format!("The Claude Code install could not start: {error}")),
                }
            } else if self.claude_install.preview().is_none()
                && self.claude_install.status()
                    == &crate::app::bootstrap_remediation::ClaudeInstallStatus::Idle
            {
                ui.label("Choose Fix for me to review the pinned install. Nothing downloads before you approve it.");
            }
        }
    };
}

pub(super) use setup_surface_methods;
