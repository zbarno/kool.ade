use super::*;

#[cfg(test)]
pub(in crate::harness::resource_bridge::npm) fn add_test_registry_entry(
    npm: &Path,
    cache: &Path,
    archive: &Path,
    registry_url: &str,
) -> anyhow::Result<()> {
    let npm_root = npm
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow::anyhow!("npm CLI has no package root"))?;
    let cacache = npm_root.join("node_modules/cacache");
    anyhow::ensure!(cacache.is_dir(), "npm cache index helper is unavailable");
    let script = r#"
const fs = require('node:fs')
const cacache = require(process.argv[1])
const [cache, url, archive] = process.argv.slice(2)
const key = `make-fetch-happen:request-cache:${url}`
const metadata = {
  time: Date.now(),
  url,
  reqHeaders: {},
  resHeaders: {
    'cache-control': 'public, max-age=31536000',
    'content-type': 'application/octet-stream',
    date: new Date().toUTCString(),
  },
  options: { compress: true },
}
cacache.put(cache, key, fs.readFileSync(archive), { metadata }).catch(error => {
  console.error(error.message)
  process.exitCode = 1
})
"#;
    let status = Command::new("node")
        .arg("--eval")
        .arg(script)
        .arg(&cacache)
        .arg(cache.join("_cacache"))
        .arg(registry_url)
        .arg(archive)
        .env_clear()
        .env("PATH", env::var_os("PATH").unwrap_or_default())
        .status()?;
    anyhow::ensure!(
        status.success(),
        "npm cache could not index its synthetic registry entry"
    );
    Ok(())
}

#[cfg(test)]
pub(in crate::harness::resource_bridge::npm) fn add_test_registry_packument(
    npm: &Path,
    cache: &Path,
    registry: &str,
    package: &str,
    version: &str,
    archive_url: &str,
    integrity: &str,
) -> anyhow::Result<()> {
    let npm_root = npm
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow::anyhow!("npm CLI has no package root"))?;
    let cacache = npm_root.join("node_modules/cacache");
    anyhow::ensure!(cacache.is_dir(), "npm cache index helper is unavailable");
    let url = format!("{}/{package}", registry.trim_end_matches('/'));
    let packument = serde_json::json!({
        "_id": package,
        "name": package,
        "dist-tags": { "latest": version },
        "versions": {
            (version): {
                "name": package,
                "version": version,
                "dist": {
                    "tarball": archive_url,
                    "integrity": integrity,
                },
            }
        },
    });
    let script = r#"
const cacache = require(process.argv[1])
const [cache, url, body] = process.argv.slice(2)
const key = `make-fetch-happen:request-cache:${url}`
const accepts = [
  ['application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*', 'application/vnd.npm.install-v1+json'],
  ['application/json', 'application/json'],
]
;(async () => {
  for (const [accept, contentType] of accepts) {
    const metadata = {
      time: Date.now(),
      url,
      reqHeaders: { accept },
      resHeaders: {
        'cache-control': 'public, max-age=31536000',
        'content-type': contentType,
        date: new Date().toUTCString(),
      },
      options: { compress: true },
    }
    await cacache.put(cache, key, Buffer.from(body), { metadata })
  }
})().catch(error => {
  console.error(error.message)
  process.exitCode = 1
})
"#;
    let status = Command::new("node")
        .arg("--eval")
        .arg(script)
        .arg(&cacache)
        .arg(cache.join("_cacache"))
        .arg(url)
        .arg(serde_json::to_string(&packument)?)
        .env_clear()
        .env("PATH", env::var_os("PATH").unwrap_or_default())
        .status()?;
    anyhow::ensure!(
        status.success(),
        "npm cache could not index synthetic package metadata"
    );
    Ok(())
}
