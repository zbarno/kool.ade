use super::super::*;
use std::collections::BTreeMap;

pub struct ApiRepository {
    pub checkout: PathBuf,
    pub remote: PathBuf,
    pub source_sha: String,
    pub refs_before: BTreeMap<String, String>,
}

pub fn setup_api_repository(sandbox: &Sandbox) -> ApiRepository {
    let checkout = sandbox.root.join("api-repo");
    let remote = sandbox.root.join("api-remote.git");
    fs::create_dir_all(&checkout).unwrap();
    sandbox.git(&checkout, &["init", "-q", "-b", "main"]);
    sandbox.git(&checkout, &["config", "user.name", "Fixture"]);
    sandbox.git(&checkout, &["config", "user.email", "fixture@example.test"]);
    fs::write(checkout.join("README.md"), "Synthetic API repository.\n").unwrap();
    sandbox.git(&checkout, &["add", "."]);
    sandbox.git(&checkout, &["commit", "-qm", "API baseline"]);
    sandbox.git(
        &sandbox.root,
        &["init", "--bare", "-q", remote.to_str().unwrap()],
    );
    let url = "ssh://git@github.com/fixture/api.git";
    sandbox.git(&checkout, &["remote", "add", "origin", url]);
    sandbox.git(&checkout, &["branch", "stable", "main"]);
    let ssh = sandbox.root.join("mapped-ssh");
    fs::write(
        &ssh,
        "#!/bin/sh\nroot=$(dirname \"$0\")\ncase \"$*\" in\n  *fixture/api.git*) remote=\"$root/api-remote.git\" ;;\n  *) remote=\"$root/remote.git\" ;;\nesac\ncase \"$*\" in\n  *git-upload-pack*) exec git-upload-pack \"$remote\" ;;\n  *git-receive-pack*) exec git-receive-pack \"$remote\" ;;\n  *) exit 2 ;;\nesac\n",
    )
    .unwrap();
    executable(&ssh);
    // SAFETY: Sandbox owns/restores this environment variable under its test lock.
    unsafe { std::env::set_var("GIT_SSH_COMMAND", &ssh) };
    sandbox.git(&checkout, &["push", "-q", "origin", "main", "stable"]);
    let source_sha = sandbox.git(&checkout, &["rev-parse", "refs/heads/main"]);
    let refs_before = refs(&remote);

    let mut manifest =
        crate::core::project_repos::ProjectManifest::load(&sandbox.planning_store).unwrap();
    manifest
        .repositories
        .push(crate::core::project_repos::Repository {
            id: "api".into(),
            role: "API code".into(),
            remote: url.into(),
            display_name: None,
        });
    sandbox
        .planning_store
        .atomic_write(
            crate::core::project_repos::PROJECT_FILE,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    let checkouts = crate::core::project_repos::RepositoryCheckoutMapV1 {
        schema_version: 1,
        checkouts: BTreeMap::from([("api".into(), checkout.clone())]),
    };
    sandbox
        .planning_store
        .atomic_write(
            "config/repository-checkouts.json",
            &serde_json::to_vec_pretty(&checkouts).unwrap(),
        )
        .unwrap();
    ApiRepository {
        checkout,
        remote,
        source_sha,
        refs_before,
    }
}

pub fn install_mapped_gh(sandbox: &Sandbox, root_remote: &Path, api_remote: &Path) {
    let shell = r##"#!/bin/sh
root='@ROOT_REMOTE@'; api='@API_REMOTE@'
repo=''; head=''; base=''; url=''
for arg in "$@"; do case "$previous" in --repo) repo="$arg";; --head) head="$arg";; --base) base="$arg";; esac; case "$arg" in https://*) url="$arg";; esac; previous="$arg"; done
case "$url" in *fixture/api/*) repo=github.com/fixture/api;; esac
remote="$root"; short=repo; destination=main; marker=merged-root
case "$repo" in */api) remote="$api"; short=api; destination=stable; marker=merged-api;; esac
case "$*" in *deployments*) echo '[]'; exit 0;; esac
if [ "$2" = list ]; then echo '[]'
elif [ "$2" = create ]; then
 printf '%s\t%s\t%s\n' "$repo" "$head" "$base" >> "$(dirname "$0")/mapped-pr-log"
 echo "$head" > "$(dirname "$0")/published-$short"
 echo "https://github.com/fixture/$short/pull/1"
elif [ "$2" = view ]; then
 head_branch=$(cat "$(dirname "$0")/published-$short")
 h=$(git --git-dir "$remote" rev-parse "refs/heads/$head_branch")
 b=$(git --git-dir "$remote" rev-parse "refs/heads/$destination")
 if [ -f "$(dirname "$0")/$marker" ]; then
  m=$(cat "$(dirname "$0")/$marker")
  printf '{"url":"https://github.com/fixture/%s/pull/1","number":1,"state":"MERGED","mergeCommit":{"oid":"%s"},"headRefOid":"%s","baseRefOid":"%s","baseRefName":"%s","mergedAt":"2099-01-01T00:00:00Z"}\n' "$short" "$m" "$h" "$b" "$destination"
 else
  printf '{"url":"https://github.com/fixture/%s/pull/1","number":1,"state":"OPEN","headRefOid":"%s","baseRefOid":"%s","baseRefName":"%s"}\n' "$short" "$h" "$b" "$destination"
 fi
else echo '{}'; fi
"##
        .replace("@ROOT_REMOTE@", &root_remote.display().to_string())
        .replace("@API_REMOTE@", &api_remote.display().to_string());
    let path = sandbox.root.join("mapped-gh");
    fs::write(&path, shell).unwrap();
    executable(&path);
    fs::copy(&path, &sandbox.gh).unwrap();
    executable(&sandbox.gh);
}

pub fn refs(remote: &Path) -> BTreeMap<String, String> {
    let output = std::process::Command::new("git")
        .args([
            "--git-dir",
            remote.to_str().unwrap(),
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let (name, sha) = line.split_once(' ').unwrap();
            (name.into(), sha.into())
        })
        .collect()
}

pub fn refs_after_merge(
    before: &BTreeMap<String, String>,
    state: &Implementation,
    destination: &str,
) -> BTreeMap<String, String> {
    let mut expected = before.clone();
    expected.insert(
        format!("refs/heads/{destination}"),
        state.verified_head.clone().unwrap(),
    );
    expected.insert(
        format!("refs/heads/{}", state.branch),
        state.verified_head.clone().unwrap(),
    );
    expected
}

fn executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
