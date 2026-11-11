use super::*;

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface) {
    egui::CollapsingHeader::new("Personal VMs")
        .id_salt("computer_use_personal_profiles")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("Encrypted personal VM profiles are isolated from disposable task sessions.");
            if ui.button("Request new personal VM (30-day retention)").clicked()
                && let Err(error) = surface.request_computer_use_personal_vm_consent(
                    crate::computer_use::mcp::ComputerUsePersonalVmChoiceV1::Create {
                        retention_days: 30,
                    },
                )
            {
                ui.colored_label(crate::ui::theme::WARNING, error);
            }
            ui.small("Create and unlock require a separate, typed owner choice. Normal provider session grants remain disposable.");
            ui.small("Passwords, MFA codes, and account sessions stay inside the VM; never enter them in agent chat.");
            match surface.computer_use_personal_profiles() {
                Ok(profiles) if profiles.is_empty() => {
                    ui.label("No personal VM profiles exist for this local user.");
                }
                Ok(profiles) => {
                    for profile in profiles {
                        ui.separator();
                        ui.label(format!("Profile {}", profile.profile_id));
                        if let Some(session_id) = profile.active_session_id.as_deref() {
                            ui.label("Unlocked in a live session; the disk remains locked to other sessions.");
                            if ui.button("Lock personal VM").clicked()
                                && let Err(error) = surface.lock_computer_use_personal_profile(
                                    &profile.profile_id,
                                    session_id,
                                )
                            {
                                ui.colored_label(crate::ui::theme::WARNING, error);
                            }
                        } else {
                            ui.label("Locked. Unlock is unavailable until the R3 security gate is enabled.");
                            if ui.button("Request unlock").clicked()
                                && let Err(error) = surface.request_computer_use_personal_vm_consent(
                                    crate::computer_use::mcp::ComputerUsePersonalVmChoiceV1::Unlock {
                                        profile_id: profile.profile_id.clone(),
                                    },
                                )
                            {
                                ui.colored_label(crate::ui::theme::WARNING, error);
                            }
                        }
                        if profile.active_session_id.is_none() {
                            let confirm_id = ui.id().with(("delete_personal_profile", &profile.profile_id));
                            let confirming = ui
                                .ctx()
                                .data(|data| data.get_temp::<bool>(confirm_id).unwrap_or(false));
                            if !confirming {
                                if ui.button("Delete encrypted profile…").clicked() {
                                    ui.ctx().data_mut(|data| data.insert_temp(confirm_id, true));
                                }
                            } else {
                                ui.horizontal(|ui| {
                                    if ui.button("Confirm delete").clicked() {
                                        match surface.delete_computer_use_personal_profile(
                                            &profile.profile_id,
                                        ) {
                                            Ok(()) => {
                                                ui.ctx().data_mut(|data| {
                                                    data.remove::<bool>(confirm_id);
                                                });
                                            }
                                            Err(error) => {
                                                ui.colored_label(crate::ui::theme::WARNING, error);
                                            }
                                        }
                                    }
                                    if ui.button("Cancel").clicked() {
                                        ui.ctx().data_mut(|data| data.remove::<bool>(confirm_id));
                                    }
                                });
                            }
                        }
                    }
                }
                Err(error) => {
                    ui.colored_label(crate::ui::theme::WARNING, error);
                }
            }
            for request in surface.computer_use_personal_vm_consent_requests() {
                ui.separator();
                match &request.choice {
                    crate::computer_use::mcp::ComputerUsePersonalVmChoiceV1::Create {
                        retention_days,
                    } => ui.label(format!("Owner request: create a profile with {retention_days}-day retention.")),
                    crate::computer_use::mcp::ComputerUsePersonalVmChoiceV1::Unlock {
                        profile_id,
                    } => ui.label(format!("Owner request: unlock profile {profile_id}.")),
                };
                ui.small(format!(
                    "Target profile {} · human owns control · network disabled · private input required",
                    request.target.profile_id
                ));
                ui.horizontal(|ui| {
                    ui.add_enabled(false, egui::Button::new("Approve"));
                    ui.small("Approval unavailable until the R3 security and human-login gate passes.");
                    if ui.button("Cancel request").clicked()
                        && let Err(error) = surface.decide_computer_use_personal_vm_consent(
                            &request.request_id,
                            false,
                        )
                    {
                        ui.colored_label(crate::ui::theme::WARNING, error);
                    }
                });
            }
            ui.small("Private input hides local display capture and action summaries during human login.");
        });
}
