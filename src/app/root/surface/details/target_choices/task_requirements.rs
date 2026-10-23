use crate::{
    persistence::execution_environment_settings::TaskCapabilityRequirements,
    ui::task_detail::TargetChoice,
};

pub(super) fn apply_task_requirements(
    choices: &mut [TargetChoice],
    requirements: TaskCapabilityRequirements,
) {
    let capability = match (requirements.requires_gpu, requirements.requires_display) {
        (true, true) => "GPU and display",
        (true, false) => "GPU",
        (false, true) => "display",
        (false, false) => return,
    };
    let reason = format!(
        "{capability} access is unavailable: supported Docker runtimes and device-grant policies are not defined. This task remains blocked."
    );
    for choice in choices {
        choice.available = false;
        choice.reason.clone_from(&reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::contract::TargetPreference;

    fn choices() -> Vec<TargetChoice> {
        [
            TargetPreference::Auto,
            TargetPreference::Local,
            TargetPreference::Worker("worker-a".into()),
        ]
        .into_iter()
        .map(|preference| TargetChoice {
            preference,
            label: "target".into(),
            available: true,
            reason: "healthy".into(),
        })
        .collect()
    }

    #[test]
    fn gpu_and_display_requirements_disable_every_target_until_supported() {
        for requirements in [
            TaskCapabilityRequirements {
                requires_gpu: true,
                requires_display: false,
            },
            TaskCapabilityRequirements {
                requires_gpu: false,
                requires_display: true,
            },
            TaskCapabilityRequirements {
                requires_gpu: true,
                requires_display: true,
            },
        ] {
            let mut choices = choices();
            apply_task_requirements(&mut choices, requirements);
            assert!(choices.iter().all(|choice| !choice.available));
            assert!(choices.iter().all(|choice| {
                choice.reason.contains("unavailable") && choice.reason.contains("blocked")
            }));
        }
    }

    #[test]
    fn ordinary_task_requirements_leave_healthy_targets_available() {
        let mut choices = choices();
        apply_task_requirements(&mut choices, TaskCapabilityRequirements::default());
        assert!(choices.iter().all(|choice| choice.available));
    }
}
