use base64::Engine as _;
use flate2::{Compression, GzBuilder};
use sha2::{Digest, Sha512};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const PACKAGE_NAME: &str = "koolade-runtime-qa-cache-probe";
const PACKAGE_VERSION: &str = "1.0.0";
const REGISTRY_URL: &str = "https://registry.npmjs.org";

pub(in crate::execution::docker::qa_container::tests::browser_rootless::task_runtime) struct NpmFixtureArchive
{
    pub archive: PathBuf,
    pub archive_url: String,
    pub registry_url: String,
}

pub(in crate::execution::docker::qa_container::tests::browser_rootless::task_runtime) fn write_lockfile_and_archive(
    target_root: &Path,
    archive_root: &Path,
) -> anyhow::Result<NpmFixtureArchive> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(target_root.join("package.json"))?)?;
    let project_name = manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("npm fixture package name is missing"))?;
    anyhow::ensure!(
        manifest
            .pointer(&format!("/dependencies/{PACKAGE_NAME}"))
            .and_then(serde_json::Value::as_str)
            == Some(PACKAGE_VERSION),
        "npm fixture must pin its cache probe dependency"
    );

    let archive_bytes = package_archive()?;
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(&archive_bytes))
    );
    fs::create_dir_all(archive_root)?;
    let archive = archive_root.join("runtime-qa-cache-probe.tgz");
    fs::write(&archive, &archive_bytes)?;
    let archive_url =
        format!("{REGISTRY_URL}/{PACKAGE_NAME}/-/{PACKAGE_NAME}-{PACKAGE_VERSION}.tgz");

    let mut packages = serde_json::Map::new();
    packages.insert(
        String::new(),
        serde_json::json!({
            "name": project_name,
            "version": "1.0.0",
            "dependencies": { (PACKAGE_NAME): PACKAGE_VERSION },
        }),
    );
    packages.insert(
        format!("node_modules/{PACKAGE_NAME}"),
        serde_json::json!({
            "version": PACKAGE_VERSION,
            "resolved": archive_url.clone(),
            "integrity": integrity.clone(),
        }),
    );
    let lockfile = serde_json::json!({
        "name": project_name,
        "version": "1.0.0",
        "lockfileVersion": 3,
        "requires": true,
        "packages": packages,
    });
    fs::write(
        target_root.join("package-lock.json"),
        serde_json::to_vec_pretty(&lockfile)?,
    )?;

    Ok(NpmFixtureArchive {
        archive,
        archive_url,
        registry_url: REGISTRY_URL.into(),
    })
}

fn package_archive() -> anyhow::Result<Vec<u8>> {
    let mut gzip = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    {
        let mut archive = tar::Builder::new(&mut gzip);
        archive.mode(tar::HeaderMode::Deterministic);
        append_file(
            &mut archive,
            "package/package.json",
            br#"{"name":"koolade-runtime-qa-cache-probe","version":"1.0.0","main":"index.js"}"#,
        )?;
        append_file(
            &mut archive,
            "package/index.js",
            b"module.exports = 'prepared npm cache was installed offline';\n",
        )?;
        archive.finish()?;
    }
    Ok(gzip.finish()?)
}

fn append_file<W: std::io::Write>(
    archive: &mut tar::Builder<W>,
    path: &str,
    contents: &[u8],
) -> anyhow::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    archive.append_data(&mut header, path, Cursor::new(contents))?;
    Ok(())
}
