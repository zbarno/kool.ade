//! Opt-in, real-Pi dogfood runs. Set PACKET_DOGFOOD_SCENARIO=A..H to execute
//! one scenario against the configured provider; ordinary test runs skip it.
use super::*;
use std::path::{Path, PathBuf};
#[path = "dogfood/assertions.rs"]
mod assertions;
#[path = "dogfood/continuation.rs"]
mod continuation;

#[derive(Clone, Copy)]
struct Scenario {
    code: &'static str,
    kind: &'static str,
    request: &'static str,
}

struct LocalPiDogfood;

impl crate::harness::AiHarness for LocalPiDogfood {
    fn label(&self) -> String {
        crate::harness::PiHarness.label()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        crate::harness::PiHarness.check_available()
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        let mut request = request.clone();
        request.reasoning_level =
            std::env::var("PACKET_DOGFOOD_REASONING").unwrap_or_else(|_| "off".into());
        crate::harness::PiHarness.execute(&request)
    }
}

fn scenario(code: &str) -> Option<Scenario> {
    Some(match code {
        "A" => Scenario {
            code: "A",
            kind: "Feature",
            request: "Add the ability to export planning tasks as Markdown.",
        },
        "B" => Scenario {
            code: "B",
            kind: "Feature",
            request: "Add user accounts.",
        },
        "C" => Scenario {
            code: "C",
            kind: "Bug",
            request: "The task detail modal cuts off long blocker messages.",
        },
        "D" => Scenario {
            code: "D",
            kind: "Question",
            request: "Why does Packet use Bubblewrap?",
        },
        "E" => Scenario {
            code: "E",
            kind: "Question",
            request: "Why can't Packet use hosted Anthropic through Pi?",
        },
        "F" => Scenario {
            code: "F",
            kind: "Feature",
            request: "Add notification preferences. Storage choice, notification behavior, and theme choice are three independent decisions that can be made separately.",
        },
        "G" => Scenario {
            code: "G",
            kind: "Feature",
            request: "Add authenticated user profiles. Authentication approach depends on the storage choice; do not surface or decide authentication until storage is resolved.",
        },
        "H" => Scenario {
            code: "H",
            kind: "Feature",
            request: "Add a compact keyboard shortcut for opening task details. Inspect existing input handling and choose the shortcut convention from repository evidence; ask me only if a product decision is unavoidable.",
        },
        _ => return None,
    })
}

fn initialize_repository(root: &Path, scenario: Scenario) {
    std::fs::create_dir_all(root).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Packet Dogfood"],
        vec!["config", "user.email", "packet-dogfood@example.invalid"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    // Source-grounded scenarios get a clean read-only snapshot of the current
    // checkout. Packet writes only planning artifacts in the temporary repo.
    if matches!(scenario.code, "A" | "C" | "D" | "E" | "H") {
        let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for relative in ["README.md", "Cargo.toml", "src"] {
            copy_tree(&checkout.join(relative), &root.join(relative));
        }
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    if source.is_dir() {
        std::fs::create_dir_all(destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            copy_tree(&entry.path(), &destination.join(entry.file_name()));
        }
    } else if source.is_file() {
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::copy(source, destination).unwrap();
    }
}

fn complete_live_turn(app: &mut PacketApp, ctx: &egui::Context) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    while app.conversation_busy() {
        app.tick(0.016, ctx);
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    for _ in 0..4 {
        app.tick(0.016, ctx);
    }
    true
}

fn complete_live_task_turn(app: &mut PacketApp, ctx: &egui::Context, key: &str) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    while app.task_chat_active(key) {
        app.tick(0.016, ctx);
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    for _ in 0..4 {
        app.tick(0.016, ctx);
    }
    true
}

fn create_task_from_board(app: &mut PacketApp, ctx: &egui::Context, scenario: Scenario) {
    frame(app, ctx, vec![]);
    click_text(app, ctx, "+ New Task");
    click_text(app, ctx, scenario.kind);
    click_text(app, ctx, "Describe what you want to do…");
    frame(app, ctx, vec![egui::Event::Text(scenario.request.into())]);
    click_text(app, ctx, "Create Task");
    assert!(
        app.conversation_busy(),
        "New Task starts real planning at once"
    );
}

#[test]
fn live_dogfood_scenario_from_environment() {
    let Ok(code) = std::env::var("PACKET_DOGFOOD_SCENARIO") else {
        return;
    };
    let scenario =
        scenario(code.trim()).unwrap_or_else(|| panic!("unsupported dogfood scenario: {code}"));
    let resume_root = std::env::var_os("PACKET_DOGFOOD_RESUME_A_ROOT").map(PathBuf::from);
    let root = resume_root.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "packet-live-dogfood-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    });
    if resume_root.is_none() {
        initialize_repository(&root, scenario);
    }
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
        let operator = project.state.effective_user().name;
        for category in crate::domain::DEFAULT_CATEGORIES {
            project
                .state
                .config
                .stakeholders
                .upsert(crate::domain::CategoryOwners::new(
                    *category,
                    vec![operator.clone()],
                ));
        }
        for category in ["Engineering", "Architecture"] {
            project
                .state
                .config
                .stakeholders
                .upsert(crate::domain::CategoryOwners::new(
                    category,
                    vec![operator.clone()],
                ));
        }
        let config_path = crate::artifacts::repo_artifact(&root, crate::artifacts::CONFIG_FILE);
        crate::artifacts::atomic_write(
            &config_path,
            &crate::artifacts::config_io::serialize(&project.state.config),
        )
        .unwrap();
        project.chat_slug = root.join("runtime").to_string_lossy().into_owned();
        project.task_documents = if resume_root.is_some() {
            crate::artifacts::task_docs::load_board(
                &project.state.repo_root,
                &project.state.workflow,
            )
        } else {
            Vec::new()
        };
    }
    app.task_harness = Some(Box::new(LocalPiDogfood));
    let ctx = egui::Context::default();
    if resume_root.is_some() {
        let evidence = continuation::approve_and_prepare_scenario_a(&mut app, &ctx, &root);
        let evidence_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".kool-ade-packet/planning/tasks/packet-task-centric-ux-remediation/evidence/scenario-a-continuation-live.json");
        std::fs::write(
            &evidence_path,
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
        eprintln!(
            "PACKET_LIVE_DOGFOOD_A_CONTINUATION={}",
            serde_json::to_string(&evidence).unwrap()
        );
        return;
    }
    create_task_from_board(&mut app, &ctx, scenario);
    let completed = complete_live_turn(&mut app, &ctx);
    let initial_evidence = continuation::save_evidence(scenario, &root, &app, completed);
    let continuation = if completed
        && scenario.code == "A"
        && std::env::var("PACKET_DOGFOOD_CONTINUE_A").as_deref() == Ok("1")
    {
        Some(continuation::continue_scenario_a(&mut app, &ctx, &root))
    } else {
        None
    };
    let evidence = continuation
        .as_ref()
        .map(|run| serde_json::json!({"initial": initial_evidence, "continuation": run}))
        .unwrap_or(initial_evidence);
    let evidence_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".kool-ade-packet/planning/tasks/packet-task-centric-ux-remediation/evidence")
        .join(format!(
            "scenario-{}-live.json",
            scenario.code.to_ascii_lowercase()
        ));
    if continuation.is_some() {
        std::fs::write(
            &evidence_path,
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
    eprintln!(
        "PACKET_LIVE_DOGFOOD={}",
        serde_json::to_string(&evidence).unwrap()
    );

    let saved = crate::core::planning_work::load(&root).unwrap();
    let work = saved.last().unwrap();
    if completed {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assertions::check_outcome(scenario, &root, work, &evidence);
        }));
        if !app.conversation_busy() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        result.unwrap();
    }
    assert!(
        completed,
        "live Pi dogfood turn exceeded ten minutes; partial evidence was saved"
    );
}
