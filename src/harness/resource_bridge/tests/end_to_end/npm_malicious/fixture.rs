use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(super) fn create_archive(
    workspace: &Path,
    repository: &Path,
    probe_host: &str,
    probe_port: u16,
) -> PathBuf {
    let package_dir = repository.join("fixtures/archive-helper");
    fs::create_dir_all(&package_dir).unwrap();
    fs::write(
        package_dir.join("package.json"),
        r#"{"name":"archive-helper","version":"1.0.0","main":"index.js","scripts":{"postinstall":"node postinstall.js"}}"#,
    )
    .unwrap();
    fs::write(package_dir.join("index.js"), "module.exports = 'ready';\n").unwrap();
    let probe_host = serde_json::to_string(probe_host).unwrap();
    let postinstall = format!(
        r#"const fs = require('node:fs');
const net = require('node:net');
let finished = false;
const socket = net.createConnection({{ host: {probe_host}, port: {probe_port} }});
const record = (network) => {{
  if (finished) return;
  finished = true;
  socket.destroy();
  fs.writeFileSync(require('node:path').join(__dirname, 'install-probe.json'), JSON.stringify({{
    network,
    hostSecretPresent: Object.hasOwn(process.env, 'KOOLADE_E2E_HOST_SECRET'),
  }}));
}};
socket.setTimeout(750);
socket.on('connect', () => record('connected'));
socket.on('error', () => record('blocked'));
socket.on('timeout', () => record('blocked'));
"#
    );
    fs::write(package_dir.join("postinstall.js"), postinstall).unwrap();

    let global_config = workspace.join("npm-globalrc");
    fs::write(&global_config, []).unwrap();
    let npm_home = workspace.join("npm-home");
    fs::create_dir_all(&npm_home).unwrap();
    let output = Command::new("npm")
        .args([
            &format!("--globalconfig={}", global_config.display()),
            "--userconfig=/dev/null",
            "--offline",
            "--ignore-scripts",
            "pack",
            "./fixtures/archive-helper",
        ])
        .arg(format!("--pack-destination={}", workspace.display()))
        .current_dir(repository)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &npm_home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "local npm fixture setup failed: {}; {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let archive = workspace.join("archive-helper-1.0.0.tgz");
    assert!(
        archive.is_file(),
        "npm pack did not create the fixture archive"
    );
    archive
}
