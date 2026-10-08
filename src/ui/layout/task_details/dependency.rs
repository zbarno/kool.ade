use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    request: &crate::harness::DependencyRequest,
) {
    let awaiting = request.status == crate::harness::DependencyRequestStatus::AwaitingUser;
    egui::Frame::NONE
        .fill(egui::Color32::from_rgb(43, 36, 22))
        .stroke(egui::Stroke::new(1.0, theme::WARNING))
        .corner_radius(8)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.label(
                RichText::new(if awaiting {
                    "DEPENDENCY AUTHORIZATION REQUIRED"
                } else {
                    "DEPENDENCY REQUEST"
                })
                .size(10.5)
                .strong()
                .color(theme::WARNING),
            );
            let package = request
                .need
                .package
                .as_deref()
                .unwrap_or("package not identified");
            let version = request
                .need
                .version
                .as_deref()
                .map(|version| format!("@{version}"))
                .unwrap_or_default();
            ui.label(
                RichText::new(format!("{package}{version}"))
                    .monospace()
                    .strong(),
            );
            ui.label(format!(
                "Ecosystem: {:?} · Source: {}",
                request.need.ecosystem,
                request.need.source.as_deref().unwrap_or("not specified")
            ));
            ui.label(format!("Category: {}", category_label(request.category)));
            ui.label(format!("Requested by task {}", request.task_id));
            ui.add(egui::Label::new(format!("Reason: {}", request.need.reason)).wrap());
            for package in &request.need.introduced_packages {
                ui.add(
                    egui::Label::new(format!(
                        "Added since task start: {}@{} · {} · {}",
                        package.package, package.version, package.source, package.integrity
                    ))
                    .wrap(),
                );
            }
            ui.add(egui::Label::new(format!("Risk: {}", request.risk)).wrap());
            ui.add(egui::Label::new(&request.rationale).wrap());
            ui.collapsing("Requested command", |ui| {
                reply::full_message(ui, &request.need.command, "dependency_command");
            });
            if awaiting {
                let can_authorize_once = crate::harness::dependency_decision_allowed(
                    &request.need,
                    crate::harness::DependencyDecision::UserAuthorizeForTask,
                );
                let can_authorize_project = crate::harness::dependency_decision_allowed(
                    &request.need,
                    crate::harness::DependencyDecision::UserAuthorizeForProject,
                );
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(can_authorize_once, egui::Button::new("Authorize once"))
                        .clicked()
                    {
                        super::details::dispatch(
                            surface,
                            crate::ui::task_detail::Command::AuthorizeDependency {
                                ticket: ticket.to_owned(),
                                request_id: request.id.clone(),
                                scope: crate::harness::DependencyAuthorizationScope::Once,
                            },
                        );
                    }
                    if ui
                        .add_enabled(
                            can_authorize_project,
                            egui::Button::new("Authorize for project"),
                        )
                        .clicked()
                    {
                        super::details::dispatch(
                            surface,
                            crate::ui::task_detail::Command::AuthorizeDependency {
                                ticket: ticket.to_owned(),
                                request_id: request.id.clone(),
                                scope: crate::harness::DependencyAuthorizationScope::Project,
                            },
                        );
                    }
                    if ui.button("Deny").clicked() {
                        super::details::dispatch(
                            surface,
                            crate::ui::task_detail::Command::DenyDependency {
                                ticket: ticket.to_owned(),
                                request_id: request.id.clone(),
                            },
                        );
                    }
                });
                if !can_authorize_once && !can_authorize_project {
                    ui.add(
                        egui::Label::new("Kool.ad/e's current broker cannot safely prepare this source or ecosystem, so authorization cannot enable it.")
                            .wrap(),
                    );
                }
            } else {
                ui.label(format!("Status: {:?}", request.status));
            }
        });
}

fn category_label(category: crate::harness::DependencyFailureCategory) -> &'static str {
    use crate::harness::DependencyFailureCategory as Category;
    match category {
        Category::Unknown => "Unclassified dependency request",
        Category::DependencyMissing => "Dependency unavailable",
        Category::DependencyRestoreRequired => "Project restore required",
        Category::DependencyNewPackageRequested => "New package requested",
        Category::DependencySourceNotAuthorized => "Source needs authorization",
        Category::DependencyManagerUnsupported => "Package manager unsupported",
        Category::DependencyIntegrityFailure => "Package integrity could not be verified",
        Category::DependencyPrivateRegistry => "Private registry requested",
        Category::DependencySystemPackageRequired => "System tool required",
        Category::DependencyPolicyDenied => "Dependency request denied by policy",
    }
}
