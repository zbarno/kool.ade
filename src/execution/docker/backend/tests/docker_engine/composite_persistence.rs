//! Opt-in Docker integration coverage for persisted, content-addressed
//! composite runtimes (issue #529 completion).
//!
//! Production boundary: the full local connect path
//! (`connect_local_with_tools` -> verified image-lock selection -> offline
//! composite preparation -> non-root, network-denied execution container)
//! against a real rootless Docker daemon and the locked runtime images.
//! Unit tests cannot prove that a composite image built by one application
//! lifetime is rediscovered and reused by the next one, that concurrent
//! preparers converge on one image, or that a foreign image squatting on the
//! content-addressed derived tag is handled conservatively.
//!
//! External dependencies: rootless Docker on a Linux host, network access to
//! the locked image registry (opt-in only); the refusal scenario constructs
//! its foreign image locally via `docker import` (no third-party pull).
//! Setup cost: up to four small offline composite builds (one per scenario)
//! across the three tests, each dominated by toolchain layer assembly; a
//! warmed cache makes subsequent runs fast.

use super::*;
use std::time::Instant;
use support::{committed_workspace, native_docker_arch};

use crate::execution::{
    contract::{ExecutionModeV1, ResourceBudgetV1},
    docker::images::{ImageLock, ProjectRuntimeImagePlan, RuntimeProfile},
    session::ExecutionSession,
};

const CROSS_TOOL_PROOF: &str = "set -eu\ncommand -v cargo >/dev/null\ncommand -v node >/dev/null\ncargo --version\nnode -e 'console.log(JSON.stringify({ok:true}))'";

fn opted_in_on_linux() -> bool {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping composite persistence integration; set {OPT_IN}=1 to opt in");
        return false;
    }
    if !cfg!(target_os = "linux") {
        eprintln!("skipping composite persistence integration; requires native Linux");
        return false;
    }
    true
}

fn docker_run(args: &[&str]) -> String {
    let output = std::process::Command::new("docker")
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("spawn docker for composite integration: {error}"));
    assert!(
        output.status.success(),
        "docker {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn normalized_image_id(raw: &str) -> String {
    if raw.starts_with("sha256:") {
        raw.to_owned()
    } else {
        format!("sha256:{raw}")
    }
}

struct ReferenceCounters {
    digest_count: u32,
}

fn reference_counters(image_id: &str) -> ReferenceCounters {
    let counters = docker_run(&[
        "image",
        "inspect",
        "--format",
        "{{len .RepoTags}} {{len .RepoDigests}}",
        image_id,
    ]);
    let (_tag_part, digest_part) = counters
        .split_once(' ')
        .unwrap_or_else(|| panic!("unexpected counter output from image inspect: {counters:?}"));
    ReferenceCounters {
        digest_count: digest_part
            .parse()
            .unwrap_or_else(|_| panic!("bad digest count {digest_part:?}")),
    }
}

/// The locked foundations carry digest-pinned identities, not tags. Anchor
/// any tag-less foundation under a synthetic test reference so a routine
/// operator `docker image prune` of dangling images cannot destroy state
/// the rest of the suite depends on. Idempotent.
fn anchor_tag_less_foundations() {
    let lock = ImageLock::bundled().expect("reviewed runtime image lock");
    let arch = native_docker_arch();
    for profile in [
        RuntimeProfile::General,
        RuntimeProfile::Rust,
        RuntimeProfile::Node,
        RuntimeProfile::Python,
        RuntimeProfile::Go,
    ] {
        let selected = match lock.select(profile, arch) {
            Ok(selected) => selected,
            Err(_) => continue,
        };
        let counters = docker_run(&[
            "image",
            "inspect",
            "--format",
            "{{len .RepoTags}} {{len .RepoDigests}}",
            &selected.digest,
        ]);
        let tagless_only = matches!(counters.as_str(), "0 0");
        if tagless_only {
            docker_run(&[
                "tag",
                &selected.digest,
                &format!("koolade-integrations:foundation-{profile:?}"),
            ]);
        }
    }
}

fn last_verified_ms(identity: &str, image_id: &str) -> Option<u64> {
    crate::execution::docker::images::CompositionReceipt::last_verified_at(identity, image_id)
}

#[test]
fn composite_runtime_is_rediscovered_after_an_application_restart() {
    if !opted_in_on_linux() {
        return;
    }
    let required = vec!["cargo".to_owned(), "node".to_owned()];
    let lock = ImageLock::bundled().expect("reviewed runtime image lock");
    let arch = native_docker_arch();
    anchor_tag_less_foundations();
    // Preconditions: this tool set must genuinely require composition, and
    // every locked profile must lack the full set, or the test degenerates.
    match lock.plan_project_runtime(arch, &required) {
        Ok(ProjectRuntimeImagePlan::Composition(candidate)) => {
            let first_started = Instant::now();
            let fixture = Fixture::new();
            let (workspace, clone) = committed_workspace(&fixture);
            let session = ExecutionSession::connect_local_with_tools(
                workspace.clone(),
                &clone,
                ExecutionModeV1::Implementation,
                &required,
                ResourceBudgetV1::default(),
            )
            .expect("first provision composes the verified composite");
            let recipe_identity = session
                .runtime_provenance()
                .as_ref()
                .and_then(|value| value.recipe_digest.clone())
                .expect("composite connection records the recipe digest");
            assert_eq!(recipe_identity, candidate.recipe_sha256);
            let first_digest = session.connected_image_digest();
            let locked: Vec<String> = [
                RuntimeProfile::General,
                RuntimeProfile::Rust,
                RuntimeProfile::Node,
                RuntimeProfile::Python,
                RuntimeProfile::Go,
            ]
            .iter()
            .map(|profile| lock.select(*profile, arch).unwrap().digest)
            .collect();
            assert!(
                !locked.contains(&first_digest),
                "a composed digest must not collide with a locked profile image"
            );
            let first = session
                .run_tool_command(CROSS_TOOL_PROOF, Duration::from_secs(180))
                .expect("composed runtime executes both tools");
            assert_eq!(first.result.exit_code, Some(0), "{:?}", first.result);
            assert!(
                first.stdout.contains("cargo "),
                "cargo must answer in container"
            );
            assert!(
                first.stdout.contains("\"ok\":true"),
                "node must answer in container"
            );
            let first_wall = first_started.elapsed();
            let first_receipt = last_verified_ms(&recipe_identity, &first_digest);
            assert!(
                first_receipt.is_some(),
                "verified receipt must be persisted"
            );
            let _ = session;

            // Second application lifetime: brand-new process-equivalent
            // session over the same state root and daemon.
            let second_started = Instant::now();
            let resumed = ExecutionSession::connect_local_with_tools(
                workspace.clone(),
                &clone,
                ExecutionModeV1::Implementation,
                &required,
                ResourceBudgetV1::default(),
            )
            .expect("restart must rediscover the persisted composite");
            let second_digest = resumed.connected_image_digest();
            assert_eq!(
                second_digest, first_digest,
                "restart must reuse the persisted verified image, not rebuild"
            );
            let second = resumed
                .run_tool_command(CROSS_TOOL_PROOF, Duration::from_secs(120))
                .expect("resumed runtime executes both tools");
            assert_eq!(second.result.exit_code, Some(0), "{:?}", second.result);
            let second_wall = second_started.elapsed();
            let _ = resumed;

            assert!(
                last_verified_ms(&recipe_identity, &second_digest).is_some(),
                "the persisted receipt must still serve the reused identity"
            );
            let second_verified = last_verified_ms(&recipe_identity, &second_digest).unwrap_or(0);
            assert!(
                second_verified >= first_receipt.unwrap_or(0),
                "restart revalidation must refresh the persisted receipt, never lose it"
            );
            assert!(
                second_wall < first_wall,
                "rediscovery ({second_wall:?}) must beat a full recomposition ({first_wall:?})"
            );
        }
        other => panic!("precondition: cargo+node must plan a composition, got {other:?}"),
    }
}

#[test]
fn concurrent_same_recipe_preparers_converge_on_one_composite() {
    if !opted_in_on_linux() {
        return;
    }
    // Deliberately a different tool set — hence a different recipe identity
    // and derived tag — from the restart test: the two integration tests may
    // run in parallel against one shared daemon.
    let required = vec!["cargo".to_owned(), "python".to_owned()];
    let fixture = Fixture::new();
    let (workspace, clone) = committed_workspace(&fixture);
    anchor_tag_less_foundations();
    let required_b = required.clone();
    let (workspace_a, workspace_b) = (workspace.clone(), workspace.clone());
    let (clone_a, clone_b) = (clone.clone(), clone.clone());
    let first = std::thread::spawn(move || {
        ExecutionSession::connect_local_with_tools(
            workspace_a,
            &clone_a,
            ExecutionModeV1::Implementation,
            &required,
            ResourceBudgetV1::default(),
        )
        .expect("concurrent composer must converge")
    });
    let second = std::thread::spawn(move || {
        ExecutionSession::connect_local_with_tools(
            workspace_b,
            &clone_b,
            ExecutionModeV1::Implementation,
            &required_b,
            ResourceBudgetV1::default(),
        )
        .expect("concurrent composer must converge")
    });
    let (first, second) = (first.join().unwrap(), second.join().unwrap());
    assert_eq!(
        first.connected_image_digest(),
        second.connected_image_digest(),
        "two preparers for the same recipe must converge on one image"
    );
    assert_eq!(
        first
            .runtime_provenance()
            .as_ref()
            .and_then(|value| value.recipe_digest.clone()),
        second
            .runtime_provenance()
            .as_ref()
            .and_then(|value| value.recipe_digest.clone()),
        "converged sessions must carry the same recipe provenance"
    );
    let proof = second
        .run_tool_command(
            "set -eu\ncommand -v cargo >/dev/null\ncommand -v python >/dev/null\npython -c 'print(2 ** 3)'",
            Duration::from_secs(120),
        )
        .expect("converged runtime executes both tools");
    assert_eq!(proof.result.exit_code, Some(0), "{:?}", proof.result);
    assert!(proof.stdout.trim().ends_with("8"), "python must evaluate");
    let _ = (&first, &second);
}

/// Planted squats on the app-owned derived tag namespace must heal
/// non-destructively. Production boundary: the Docker daemon's persistent
/// tag storage is adversarial input to every composition. Covers the two
/// dispositions the engine exposes to operators:
///
/// * an unverified foreign image squatting the tag under a single tag — the
///   exact posture ordinary `docker import` creates, because the engine mints
///   a digest reference that expires with the last tag and therefore offers
///   no real protection — is refused with an operator-actionable error and
///   survives untouched;
/// * a signed library image squatting the tag under an alias keeps its own
///   references, so the recovery path likewise releases the reservation and
///   rebuilds without touching the library.
///
/// This scenario exercises the tag-only protection rule in
/// `derived_tag_guard`: repo digest references never count as protection
/// because the Docker engine deletes an image when its last tag is removed.
///
/// Both scenarios use throwaway tool sets, so they cannot race other
/// scenarios' receipts or tags in a shared opt-in run.
#[test]
fn planted_derived_tag_squats_recover_non_destructively_and_preserve_foreign_data() {
    if !opted_in_on_linux() {
        return;
    }
    let lock = ImageLock::bundled().expect("reviewed runtime image lock");
    let arch = native_docker_arch();
    anchor_tag_less_foundations();
    let fixture = Fixture::new();
    let (workspace, task_clone) = committed_workspace(&fixture);

    // Scenario A — foreign squat: an unverified image plants itself under
    // the content-addressed derived tag for python+go.
    let refusal_tools = vec!["python".to_owned(), "go".to_owned()];
    let refusal_hash = match lock.plan_project_runtime(arch, &refusal_tools) {
        Ok(ProjectRuntimeImagePlan::Composition(candidate)) => candidate.recipe_sha256,
        other => panic!("precondition: python+go must plan a composition, got {other:?}"),
    };
    let reserved_tag = format!("koolade-derived:{}", strip_sha(&refusal_hash));
    let squatter_dir = std::env::temp_dir().join("koolade-squat-import");
    std::fs::create_dir_all(&squatter_dir).expect("scratch import dir");
    let squatter_payload = squatter_dir.join("marker");
    std::fs::write(&squatter_payload, b"foreign-content").expect("scribble import payload");
    let squatter_tar = squatter_dir.join("payload.tar");
    let packed = std::process::Command::new("tar")
        .arg("cf")
        .arg(&squatter_tar)
        .arg("-C")
        .arg(&squatter_dir)
        .arg("marker")
        .output()
        .expect("pack squatter payload");
    assert!(packed.status.success(), "tar packing failed: {:?}", packed);
    let _import_print = docker_run(&["import", squatter_tar.to_str().unwrap(), &reserved_tag]);
    // `docker import` prints a manifest digest, not the image config id; the
    // authoritative id comes from inspecting the reference the import bound.
    // Successful resolution doubles as proof the plant reached the reserved
    // tag.
    let tag_holder = docker_run(&["image", "inspect", "--format", "{{.Id}}", &reserved_tag]);
    let imported_id = normalized_image_id(&tag_holder);
    let protected_counters = reference_counters(&imported_id);
    assert!(
        protected_counters.digest_count >= 1,
        "ordinary image creation mints a digest reference that the engine lets expire with the last tag"
    );
    // The reserved tag is thus the squatter's only protection; the builder
    // must refuse to release it and keep the foreign image intact.
    let refusal = match ExecutionSession::connect_local_with_tools(
        workspace.clone(),
        &task_clone,
        ExecutionModeV1::Implementation,
        &refusal_tools,
        ResourceBudgetV1::default(),
    ) {
        Err(refusal) => refusal,
        Ok(_) => panic!("a squatting single-tag image must be refused, not deleted"),
    };
    assert!(
        refusal.message.contains("reconcile the tag manually"),
        "operator-actionable guidance expected, got: {}",
        refusal.message
    );
    let survivor = docker_run(&["image", "inspect", "--format", "{{.Id}}", &imported_id]);
    assert_eq!(
        survivor, imported_id,
        "the refused squatter image must survive untouched"
    );
    let still_planted = docker_run(&["image", "inspect", "--format", "{{.Id}}", &reserved_tag]);
    assert_eq!(
        still_planted, imported_id,
        "the planted tag must remain untouched after refusal"
    );
    // Scaffold teardown: only the test's own imported image is removed.
    let _ = docker_run(&["rmi", &imported_id]);

    // Scenario B — signed library alias: the Rust foundation squats under
    // the cargo+go derived tag while keeping its library references.
    let recovery_tools = vec!["cargo".to_owned(), "go".to_owned()];
    let recovery_hash = match lock.plan_project_runtime(arch, &recovery_tools) {
        Ok(ProjectRuntimeImagePlan::Composition(candidate)) => candidate.recipe_sha256,
        other => panic!("precondition: cargo+go must plan a composition, got {other:?}"),
    };
    let alien_profile = lock
        .select(RuntimeProfile::Rust, arch)
        .expect("locked profile image");
    docker_run(&[
        "tag",
        &alien_profile.digest,
        &format!("koolade-derived:{}", strip_sha(&recovery_hash)),
    ]);
    let recovered = ExecutionSession::connect_local_with_tools(
        workspace,
        &task_clone,
        ExecutionModeV1::Implementation,
        &recovery_tools,
        ResourceBudgetV1::default(),
    )
    .expect("aliased squat must be released and the recipe rebuilt");
    let recovered_digest = recovered.connected_image_digest();
    assert_ne!(
        recovered_digest, alien_profile.digest,
        "the connected runtime must be the freshly built composite"
    );
    let library_tags = docker_run(&[
        "image",
        "inspect",
        "--format",
        "{{range .RepoTags}}{{.}} {{end}}",
        &alien_profile.digest,
    ]);
    assert!(
        !library_tags.contains(strip_sha(&recovery_hash)),
        "only the app-owned derived tag may be released, got: {library_tags}"
    );
    let proof = recovered
        .run_tool_command("set -eu\ngo version", Duration::from_secs(180))
        .expect("scenario B composite executes go");
    assert_eq!(proof.result.exit_code, Some(0), "{:#?}", proof.result);
}
fn strip_sha(identity: &str) -> &str {
    identity.strip_prefix("sha256:").unwrap_or(identity)
}
