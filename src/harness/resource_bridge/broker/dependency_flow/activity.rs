use super::BrokerContext;
use crate::harness::DependencyRequest;

pub(super) fn set_dependency_request(context: &BrokerContext<'_>, request: &mut DependencyRequest) {
    if matches!(
        request.status,
        crate::harness::DependencyRequestStatus::AwaitingUser
            | crate::harness::DependencyRequestStatus::Authorized
            | crate::harness::DependencyRequestStatus::Denied
            | crate::harness::DependencyRequestStatus::Prepared
            | crate::harness::DependencyRequestStatus::Failed
    ) {
        let preparation = request
            .preparation
            .get_or_insert_with(crate::harness::DependencyPreparationTelemetry::default);
        if request.status == crate::harness::DependencyRequestStatus::AwaitingUser {
            preparation.status =
                Some(crate::harness::DependencyPreparationStatus::AuthorizationRequired);
        } else if request.status == crate::harness::DependencyRequestStatus::Denied {
            preparation.status = Some(crate::harness::DependencyPreparationStatus::Denied);
        } else if request.status == crate::harness::DependencyRequestStatus::Authorized {
            preparation.authorization_source = authorization_source(request.decision);
            preparation.status = None;
        } else if request.status == crate::harness::DependencyRequestStatus::Prepared
            && preparation.status.is_none()
        {
            preparation.status = Some(crate::harness::DependencyPreparationStatus::Prepared);
        } else if request.status == crate::harness::DependencyRequestStatus::Failed
            && preparation.status.is_none()
        {
            preparation.status = Some(match request.category {
                crate::harness::DependencyFailureCategory::DependencyPrivateRegistry => {
                    crate::harness::DependencyPreparationStatus::CredentialsRequired
                }
                crate::harness::DependencyFailureCategory::DependencyIntegrityFailure => {
                    crate::harness::DependencyPreparationStatus::IntegrityFailure
                }
                crate::harness::DependencyFailureCategory::DependencySourceNotAuthorized => {
                    crate::harness::DependencyPreparationStatus::SourceRejected
                }
                crate::harness::DependencyFailureCategory::DependencyManagerUnsupported => {
                    crate::harness::DependencyPreparationStatus::Unsupported
                }
                crate::harness::DependencyFailureCategory::DependencyPolicyDenied => {
                    crate::harness::DependencyPreparationStatus::Denied
                }
                _ => crate::harness::DependencyPreparationStatus::Error,
            });
        }
    }
    if let Ok(mut pending) = context.dependency.lock() {
        *pending = Some(request.clone());
    }
    let activity = match request.status {
        crate::harness::DependencyRequestStatus::ManagerReviewing => {
            "Man.ager is reviewing a dependency request"
        }
        crate::harness::DependencyRequestStatus::AwaitingUser => {
            "Dependency authorization needs your decision"
        }
        crate::harness::DependencyRequestStatus::Authorized => {
            "Dependency authorization was granted"
        }
        crate::harness::DependencyRequestStatus::Prepared => {
            "Authorized dependency preparation completed"
        }
        crate::harness::DependencyRequestStatus::Denied => "Dependency authorization was denied",
        crate::harness::DependencyRequestStatus::Pending => "Dependency request received",
        crate::harness::DependencyRequestStatus::Failed => "Dependency preparation needs attention",
    };
    let _ = context.progress.send(crate::harness::LiveProgress {
        activity: Some(activity.into()),
        dependency_requests: vec![request.clone()],
        ..Default::default()
    });
}

pub(super) fn authorization_source(
    decision: crate::harness::DependencyDecision,
) -> Option<crate::harness::DependencyAuthorizationSource> {
    match decision {
        crate::harness::DependencyDecision::AutoAuthorize => {
            Some(crate::harness::DependencyAuthorizationSource::Automatic)
        }
        crate::harness::DependencyDecision::AuthorizeForTask
        | crate::harness::DependencyDecision::AuthorizeForProject => {
            Some(crate::harness::DependencyAuthorizationSource::Manager)
        }
        crate::harness::DependencyDecision::UserAuthorizeForTask
        | crate::harness::DependencyDecision::UserAuthorizeForProject => {
            Some(crate::harness::DependencyAuthorizationSource::User)
        }
        crate::harness::DependencyDecision::RequiresUserAuthorization
        | crate::harness::DependencyDecision::Reject => None,
    }
}
