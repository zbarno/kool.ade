use crate::execution::docker::images::{ImageLock, RuntimeProfile};
use std::{
    net::Ipv4Addr,
    process::{Command, Output},
    thread,
    time::Duration,
};

const PORT: u16 = 38_421;
const SERVER_SCRIPT: &str = "const net=require('node:net');net.createServer(s=>s.end()).listen(Number(process.argv[1]),'0.0.0.0');";
const CLIENT_SCRIPT: &str = "const net=require('node:net');const c=net.createConnection({host:process.argv[1],port:Number(process.argv[2])});c.setTimeout(1500);c.on('connect',()=>c.end());c.on('error',e=>{console.error(e.message);process.exit(1)});c.on('timeout',()=>{console.error('connection timed out');c.destroy();process.exit(1)});";

pub(super) struct BridgeNetworkProbe {
    name: String,
    image: String,
    host: String,
    started: bool,
}

impl BridgeNetworkProbe {
    pub(super) fn start() -> Self {
        let image = ImageLock::bundled()
            .unwrap()
            .select(RuntimeProfile::Node, "amd64")
            .unwrap()
            .reference
            .clone();
        let name = format!("koolade-npm-network-probe-{}", uuid::Uuid::new_v4());
        let mut probe = Self {
            name,
            image,
            host: String::new(),
            started: false,
        };
        let port = PORT.to_string();
        let output = docker()
            .args([
                "run",
                "--pull=never",
                "--detach",
                "--rm",
                "--name",
                &probe.name,
                "--network",
                "bridge",
                "--read-only",
                "--pids-limit",
                "16",
                "--memory",
                "64m",
                "--cpus",
                "0.5",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges",
                "--entrypoint",
                "node",
                &probe.image,
                "-e",
                SERVER_SCRIPT,
                &port,
            ])
            .output()
            .unwrap();
        assert_success(
            &output,
            "starting the bridge-network positive-control listener",
        );
        probe.started = true;
        probe.host = probe.inspect_address();
        probe.verify_bridge_connectivity();
        probe
    }

    pub(super) fn host(&self) -> &str {
        &self.host
    }

    pub(super) fn port(&self) -> u16 {
        PORT
    }

    fn inspect_address(&self) -> String {
        let output = docker()
            .args([
                "inspect",
                "--format",
                "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
                &self.name,
            ])
            .output()
            .unwrap();
        assert_success(
            &output,
            "inspecting the bridge-network positive-control listener",
        );
        let host = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        assert!(
            host.parse::<Ipv4Addr>().is_ok(),
            "invalid Docker bridge IP: {host}"
        );
        host
    }

    fn verify_bridge_connectivity(&self) {
        let port = PORT.to_string();
        let mut last_output = None;
        for _ in 0..20 {
            let output = docker()
                .args([
                    "run",
                    "--pull=never",
                    "--rm",
                    "--network",
                    "bridge",
                    "--read-only",
                    "--pids-limit",
                    "16",
                    "--memory",
                    "64m",
                    "--cpus",
                    "0.5",
                    "--cap-drop",
                    "ALL",
                    "--security-opt",
                    "no-new-privileges",
                    "--entrypoint",
                    "node",
                    &self.image,
                    "-e",
                    CLIENT_SCRIPT,
                    &self.host,
                    &port,
                ])
                .output()
                .unwrap();
            if output.status.success() {
                return;
            }
            last_output = Some(output);
            thread::sleep(Duration::from_millis(100));
        }
        assert_success(
            last_output.as_ref().expect("the positive control ran"),
            "verifying bridge-network connectivity",
        );
    }
}

impl Drop for BridgeNetworkProbe {
    fn drop(&mut self) {
        if !self.started {
            return;
        }
        let _ = docker().args(["rm", "--force", &self.name]).output();
    }
}

fn docker() -> Command {
    let mut command = Command::new("docker");
    command.env_remove("KOOLADE_E2E_HOST_SECRET");
    command
}

fn assert_success(output: &Output, action: &str) {
    assert!(
        output.status.success(),
        "failed {action}: status={:?}; stdout={}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
