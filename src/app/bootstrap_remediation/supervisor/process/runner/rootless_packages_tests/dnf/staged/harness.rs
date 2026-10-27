use std::time::Duration;

use crate::app::bootstrap_remediation::{ProcessOutput, ProcessRunError, TrustedProgram};

use super::super::super::super::dnf::execute_sequence;
use super::super::super::{ScriptedRunner, dnf_artifacts, dnf_candidates};

#[derive(Clone, Copy, Default)]
pub(super) struct ResponsePlan {
    pub(super) fail_at: Option<usize>,
    pub(super) truncate_at: Option<usize>,
    pub(super) extra_key: bool,
    pub(super) invalid_manifest: bool,
    pub(super) changed_second_manifest: bool,
    pub(super) changed_second_hash: bool,
    pub(super) installed_names: &'static str,
}

pub(super) fn runner(plan: ResponsePlan) -> ScriptedRunner {
    ScriptedRunner::with_responder(move |index, program, arguments| {
        if plan.fail_at == Some(index) {
            return output(100, "");
        }
        let mut response = match (program, index) {
            (TrustedProgram::Dnf, 3) => output(0, dnf_candidates()),
            (TrustedProgram::Find, 6) => manifest(plan.invalid_manifest, false),
            (TrustedProgram::Find, 14) => manifest(false, plan.changed_second_manifest),
            (TrustedProgram::Sha256sum, 7 | 15) => {
                let path = arguments.last().expect("hash command has an RPM path");
                let digest = if plan.changed_second_hash && index == 15 {
                    "b".repeat(64)
                } else {
                    "a".repeat(64)
                };
                output(0, &format!("{digest} *{path}\n"))
            }
            (TrustedProgram::RpmKeys, 10 | 16) => key_inventory(plan.extra_key && index == 10),
            (TrustedProgram::RpmKeys, 11) => output(0, "staged RPM signature and digests OK\n"),
            (TrustedProgram::Rpm, 12) => output(0, "docker-ce\n"),
            (TrustedProgram::Rpm, 13) => output(0, plan.installed_names),
            _ => output(0, ""),
        };
        if plan.truncate_at == Some(index) {
            response.stdout_truncated = true;
        }
        response
    })
}

pub(super) fn execute(
    runner: &ScriptedRunner,
    repository_ready: impl Fn() -> anyhow::Result<()>,
) -> Result<ProcessOutput, ProcessRunError> {
    execute_sequence(
        runner,
        Duration::from_secs(60),
        crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Fedora45Amd64,
        dnf_artifacts(),
        repository_ready,
        &crate::app::bootstrap_remediation::Cancellation::new(),
    )
}

fn output(exit_code: i32, stdout: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code,
        stdout: stdout.to_owned(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

fn manifest(invalid: bool, changed: bool) -> ProcessOutput {
    let fields = [
        if changed {
            "changed-name.rpm"
        } else {
            "docker-ce-1.0-1.x86_64.rpm"
        },
        if invalid { "l" } else { "f" },
        "0",
        "0",
        "600",
        if invalid { "0" } else { "123" },
        "1",
        "",
    ];
    output(0, &fields.join("\0"))
}

fn key_inventory(extra_key: bool) -> ProcessOutput {
    use crate::app::bootstrap_remediation::trusted_assets::{
        DOCKER_DNF_KEY_FINGERPRINT, FEDORA_DNF_KEY_FINGERPRINT,
    };

    let mut value = format!(
        "{DOCKER_DNF_KEY_FINGERPRINT} Docker signing key public key\n\
         {FEDORA_DNF_KEY_FINGERPRINT} Fedora 45 signing key public key\n"
    );
    if extra_key {
        value.push_str(&format!(
            "{} Extra signing key public key\n",
            "A".repeat(40)
        ));
    }
    output(0, &value)
}
