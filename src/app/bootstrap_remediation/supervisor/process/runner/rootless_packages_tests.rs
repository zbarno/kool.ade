use std::{cell::RefCell, collections::VecDeque};

use crate::app::bootstrap_remediation::{
    PackageSourceArtifacts, ProcessOutput, ProcessRunError, ProcessRunFailure, TrustedInvocation,
    TrustedProgram, VerifiedArtifact,
    trusted_assets::{
        DOCKER_APT_KEY, DOCKER_APT_KEY_SHA256, DOCKER_APT_SOURCES, DOCKER_APT_SOURCES_SHA256,
        DOCKER_DNF_KEY, DOCKER_DNF_KEY_SHA256, DOCKER_DNF_REPO, DOCKER_DNF_REPO_SHA256,
    },
};

use super::ElevatedCommandRunner;

const APT_SOURCE_OPTIONS: [&str; 14] = [
    "--option=RootDir=/",
    "--option=Dir::Etc::sourcelist=/etc/apt/sources.list",
    "--option=Dir::Etc::sourceparts=/etc/apt/sources.list.d",
    "--option=Dir::State::lists=/var/lib/apt/lists",
    "--option=APT::Get::List-Cleanup=true",
    "--option=APT::Architecture=amd64",
    "--option=APT::Get::AllowUnauthenticated=false",
    "--option=Acquire::AllowInsecureRepositories=false",
    "--option=Acquire::AllowWeakRepositories=false",
    "--option=Acquire::AllowDowngradeToInsecureRepositories=false",
    "--option=Acquire::Check-Date=true",
    "--option=Acquire::Check-Valid-Until=true",
    "--option=Acquire::https::Verify-Peer=true",
    "--option=Acquire::https::Verify-Host=true",
];

const DNF_COMMON_OPTIONS: [&str; 35] = [
    "--quiet",
    "--releasever=45",
    "--forcearch=x86_64",
    "--setopt=reposdir=/etc/yum.repos.d,/usr/share/dnf5/repos.d",
    "--setopt=cachedir=/var/cache/libdnf5",
    "--setopt=system_cachedir=/var/cache/libdnf5",
    "--setopt=plugins=0",
    "--setopt=gpgcheck=1",
    "--setopt=sslverify=1",
    "--setopt=sslcacert=",
    "--setopt=proxy_sslverify=1",
    "--setopt=proxy_sslcacert=",
    "--setopt=skip_if_unavailable=False",
    "--setopt=fedora.gpgcheck=1",
    "--setopt=fedora.sslverify=1",
    "--setopt=fedora.sslcacert=",
    "--setopt=fedora.proxy_sslverify=1",
    "--setopt=fedora.proxy_sslcacert=",
    "--setopt=fedora.skip_if_unavailable=False",
    "--setopt=updates.gpgcheck=1",
    "--setopt=updates.sslverify=1",
    "--setopt=updates.sslcacert=",
    "--setopt=updates.proxy_sslverify=1",
    "--setopt=updates.proxy_sslcacert=",
    "--setopt=updates.skip_if_unavailable=False",
    "--setopt=koolade-docker-ce-stable.gpgcheck=1",
    "--setopt=koolade-docker-ce-stable.sslverify=1",
    "--setopt=koolade-docker-ce-stable.sslcacert=",
    "--setopt=koolade-docker-ce-stable.proxy_sslverify=1",
    "--setopt=koolade-docker-ce-stable.proxy_sslcacert=",
    "--setopt=koolade-docker-ce-stable.skip_if_unavailable=False",
    "--disablerepo=*",
    "--enablerepo=koolade-docker-ce-stable",
    "--enablerepo=fedora",
    "--enablerepo=updates",
];

fn dnf_common_arguments() -> Vec<String> {
    DNF_COMMON_OPTIONS
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect()
}

#[path = "rootless_packages_tests/apt.rs"]
mod apt;
#[path = "rootless_packages_tests/dnf.rs"]
mod dnf;

type ScriptedResponder = dyn Fn(usize, TrustedProgram, &[String]) -> ProcessOutput;

struct ScriptedRunner {
    outputs: RefCell<VecDeque<ProcessOutput>>,
    calls: RefCell<Vec<(TrustedProgram, Vec<String>)>>,
    responder: Option<Box<ScriptedResponder>>,
}

impl ScriptedRunner {
    fn new(exit_codes: impl IntoIterator<Item = i32>) -> Self {
        Self {
            outputs: RefCell::new(exit_codes.into_iter().map(output).collect()),
            calls: RefCell::new(Vec::new()),
            responder: None,
        }
    }

    fn with_stdout(
        exit_codes: impl IntoIterator<Item = i32>,
        output_index: usize,
        stdout: &str,
    ) -> Self {
        let mut outputs = exit_codes.into_iter().map(output).collect::<Vec<_>>();
        outputs[output_index].stdout = stdout.to_owned();
        Self {
            outputs: RefCell::new(outputs.into()),
            calls: RefCell::new(Vec::new()),
            responder: None,
        }
    }

    fn with_responder(
        responder: impl Fn(usize, TrustedProgram, &[String]) -> ProcessOutput + 'static,
    ) -> Self {
        Self {
            outputs: RefCell::new(VecDeque::new()),
            calls: RefCell::new(Vec::new()),
            responder: Some(Box::new(responder)),
        }
    }
}

impl ElevatedCommandRunner for ScriptedRunner {
    fn run_elevated(
        &self,
        invocation: TrustedInvocation,
        _cancellation: &crate::app::bootstrap_remediation::Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        for argument in &invocation.arguments {
            argument.validate().unwrap();
        }
        let program = invocation.program_for_test();
        let arguments = invocation.arguments_for_test();
        let index = self.calls.borrow().len();
        self.calls.borrow_mut().push((program, arguments.clone()));
        if let Some(responder) = &self.responder {
            return Ok(responder(index, program, &arguments));
        }
        self.outputs
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))
    }
}

fn artifacts() -> PackageSourceArtifacts {
    PackageSourceArtifacts {
        create_keyrings_directory: true,
        key: Some(
            VerifiedArtifact::from_bundled_bytes(
                "bundled://docker-key-test",
                DOCKER_APT_KEY,
                DOCKER_APT_KEY_SHA256,
            )
            .unwrap(),
        ),
        sources: Some(
            VerifiedArtifact::from_bundled_bytes(
                "bundled://docker-source-test",
                DOCKER_APT_SOURCES,
                DOCKER_APT_SOURCES_SHA256,
            )
            .unwrap(),
        ),
    }
}

fn dnf_artifacts() -> PackageSourceArtifacts {
    PackageSourceArtifacts {
        create_keyrings_directory: false,
        key: Some(
            VerifiedArtifact::from_bundled_bytes(
                "bundled://docker-fedora-key-test",
                DOCKER_DNF_KEY,
                DOCKER_DNF_KEY_SHA256,
            )
            .unwrap(),
        ),
        sources: Some(
            VerifiedArtifact::from_bundled_bytes(
                "bundled://docker-fedora-repo-test",
                DOCKER_DNF_REPO,
                DOCKER_DNF_REPO_SHA256,
            )
            .unwrap(),
        ),
    }
}

fn dnf_candidates() -> &'static str {
    "docker-ce|koolade-docker-ce-stable\n\
     docker-ce-cli|koolade-docker-ce-stable\n\
     containerd.io|koolade-docker-ce-stable\n\
     docker-ce-rootless-extras|koolade-docker-ce-stable\n\
     shadow-utils|updates\n\
     slirp4netns|fedora\n"
}

fn output(exit_code: i32) -> ProcessOutput {
    ProcessOutput {
        exit_code,
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    }
}
