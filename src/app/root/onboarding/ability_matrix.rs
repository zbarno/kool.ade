use crate::persistence::setup_tasks::{LocalTask, State};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Availability {
    Available,
    Blocked,
    Unsupported,
}

struct Ability {
    label: &'static str,
    availability: Availability,
}

const VM_BASE: &[&str] = &[
    "setup:computer:kvm",
    "setup:computer:qemu",
    "setup:computer:image:amd64",
    "setup:computer:storage",
];

fn availability(
    tasks: &[LocalTask],
    checked_domains: &[String],
    required: &[&str],
) -> Availability {
    let mut all_verified = true;
    for id in required {
        match tasks
            .iter()
            .find(|task| task.id == *id)
            .map(|task| task.state)
        {
            Some(State::Unsupported) => return Availability::Unsupported,
            Some(State::Verified) => {}
            // Missing, stale, pending, or partially repaired evidence never
            // grants a capability.
            None if *id == "setup:computer:kvm"
                && checked_domains
                    .iter()
                    .any(|domain| domain == "computer:kvm") => {}
            _ => all_verified = false,
        }
    }
    if all_verified {
        Availability::Available
    } else {
        Availability::Blocked
    }
}

fn abilities(
    tasks: &[LocalTask],
    checked_domains: &[String],
    docker_ready: Option<bool>,
    scan_current: bool,
) -> [Ability; 5] {
    let mut internet = VM_BASE.to_vec();
    internet.push("setup:computer:network");
    let mut personal = VM_BASE.to_vec();
    personal.push("setup:computer:personal-encryption");
    let mut viewer = VM_BASE.to_vec();
    viewer.push("setup:computer:viewer");
    [
        Ability {
            label: "Docker coding",
            availability: if !scan_current {
                Availability::Blocked
            } else if docker_ready == Some(true) {
                Availability::Available
            } else {
                Availability::Blocked
            },
        },
        Ability {
            label: "Disposable computer",
            availability: availability(tasks, checked_domains, VM_BASE),
        },
        Ability {
            label: "Internet computer",
            availability: availability(tasks, checked_domains, &internet),
        },
        Ability {
            label: "Personal encrypted computer",
            availability: availability(tasks, checked_domains, &personal),
        },
        Ability {
            label: "Interactive viewer",
            availability: availability(tasks, checked_domains, &viewer),
        },
    ]
    .map(|mut ability| {
        if !scan_current {
            ability.availability = Availability::Blocked;
        }
        ability
    })
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    tasks: &[LocalTask],
    checked_domains: &[String],
    docker_ready: Option<bool>,
    scan_current: bool,
) {
    ui.strong("What can this computer do?");
    egui::Grid::new("computer_use_ability_matrix")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for ability in abilities(tasks, checked_domains, docker_ready, scan_current) {
                ui.label(ability.label);
                ui.label(match ability.availability {
                    Availability::Available => "Available",
                    Availability::Blocked => "Blocked · needs verification or setup",
                    Availability::Unsupported => "Unsupported on this host",
                });
                ui.end_row();
            }
        });
}

#[cfg(test)]
#[path = "ability_matrix_tests.rs"]
mod tests;
