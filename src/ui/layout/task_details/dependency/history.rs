pub fn paint_history(ui: &mut egui::Ui, requests: &[crate::harness::DependencyRequest]) {
    if requests.is_empty() {
        return;
    }
    ui.collapsing("Dependency activity", |ui| {
        for request in requests.iter().rev() {
            let heading = format!(
                "{} · {}",
                ecosystem_label(request.need.ecosystem),
                request_status_label(request.status)
            );
            ui.collapsing(heading, |ui| {
                for row in history_rows(request) {
                    ui.add(egui::Label::new(row).wrap());
                }
            });
        }
    });
}

pub(super) fn history_rows(request: &crate::harness::DependencyRequest) -> Vec<String> {
    let mut rows = vec![
        format!(
            "Operation: {} · {}",
            ecosystem_label(request.need.ecosystem),
            operation_label(request.need.kind)
        ),
        format!("Packages: {}", package_label(request)),
        format!(
            "Source: {}",
            request.need.source.as_deref().unwrap_or("not specified")
        ),
        format!("Request: {}", request_status_label(request.status)),
    ];
    if request.status == crate::harness::DependencyRequestStatus::Failed {
        rows.push(format!(
            "Classification: {}",
            super::category_label(request.category)
        ));
    }
    if let Some(preparation) = &request.preparation {
        if let Some(status) = preparation.status {
            rows.push(format!("Preparation: {}", preparation_status_label(status)));
        }
        rows.push(format!(
            "Artifacts: {} total · {} cache hits · {} downloaded · {} bytes",
            preparation.package_count,
            preparation.cache_hits,
            preparation.packages_downloaded,
            preparation.bytes_downloaded
        ));
        if let Some(source) = preparation.authorization_source {
            rows.push(format!(
                "Authorization: {}",
                authorization_source_label(source)
            ));
        }
        if let Some(retry) = preparation.retry_result {
            rows.push(format!("Offline retry: {}", retry_result_label(retry)));
        }
    }
    rows
}

fn package_label(request: &crate::harness::DependencyRequest) -> String {
    if !request.need.introduced_packages.is_empty() {
        let mut packages = request
            .need
            .introduced_packages
            .iter()
            .take(5)
            .map(|package| format!("{}@{}", package.package, package.version))
            .collect::<Vec<_>>();
        let remaining = request
            .need
            .introduced_packages
            .len()
            .saturating_sub(packages.len());
        if remaining > 0 {
            packages.push(format!("and {remaining} more"));
        }
        return packages.join(", ");
    }
    match (&request.need.package, &request.need.version) {
        (Some(package), Some(version)) => format!("{package}@{version}"),
        (Some(package), None) => package.clone(),
        (None, _) => "not specified".into(),
    }
}

fn ecosystem_label(ecosystem: crate::harness::PackageEcosystem) -> &'static str {
    use crate::harness::PackageEcosystem as Ecosystem;
    match ecosystem {
        Ecosystem::Npm => "npm",
        Ecosystem::Pnpm => "pnpm",
        Ecosystem::Yarn => "Yarn",
        Ecosystem::Cargo => "Cargo",
        Ecosystem::Nuget => "NuGet",
        Ecosystem::Pip => "pip",
        Ecosystem::Uv => "uv",
        Ecosystem::Poetry => "Poetry",
        Ecosystem::System => "System package manager",
        Ecosystem::Other => "Other package manager",
    }
}

fn operation_label(kind: crate::harness::DependencyKind) -> &'static str {
    use crate::harness::DependencyKind as Kind;
    match kind {
        Kind::ExistingRestore => "Restore existing dependencies",
        Kind::NewProjectDependency => "Add project dependency",
        Kind::DevelopmentDependency => "Add development dependency",
        Kind::SystemTool => "Request system tool",
    }
}

fn request_status_label(status: crate::harness::DependencyRequestStatus) -> &'static str {
    use crate::harness::DependencyRequestStatus as Status;
    match status {
        Status::Pending => "Pending",
        Status::ManagerReviewing => "Man.ager review",
        Status::AwaitingUser => "Authorization needed",
        Status::Authorized => "Authorized",
        Status::Denied => "Denied",
        Status::Prepared => "Prepared",
        Status::Failed => "Failed",
    }
}

fn preparation_status_label(status: crate::harness::DependencyPreparationStatus) -> &'static str {
    use crate::harness::DependencyPreparationStatus as Status;
    match status {
        Status::Prepared => "Prepared",
        Status::AlreadyAvailable => "Already available",
        Status::AuthorizationRequired => "Authorization required",
        Status::Denied => "Denied",
        Status::Unsupported => "Unsupported",
        Status::IntegrityFailure => "Integrity failure",
        Status::SourceRejected => "Source rejected",
        Status::CredentialsRequired => "Credentials required",
        Status::Error => "Error",
    }
}

fn authorization_source_label(
    source: crate::harness::DependencyAuthorizationSource,
) -> &'static str {
    use crate::harness::DependencyAuthorizationSource as Source;
    match source {
        Source::Automatic => "Automatic policy",
        Source::Manager => "Man.ager",
        Source::User => "User",
    }
}

fn retry_result_label(result: crate::harness::DependencyRetryResult) -> &'static str {
    use crate::harness::DependencyRetryResult as Result;
    match result {
        Result::Succeeded => "Succeeded",
        Result::Failed => "Failed",
    }
}
