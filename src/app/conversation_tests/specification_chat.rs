use super::*;
use crate::harness::{AiHarness, HarnessOutcome, PlanningRequest};
use std::sync::{Condvar, mpsc};

#[path = "specification_chat/continuity.rs"]
mod continuity;
#[path = "specification_chat/rejected_history.rs"]
mod rejected_history;
#[path = "specification_chat/ui_layout.rs"]
mod ui_layout;

/// Production command -> session persistence -> TurnController recovery test.
/// This crosses the dispatch and private chat-store boundaries, which a core
/// turn unit test cannot prove. It uses only a temporary Git fixture; no
/// provider, Docker, SSH, or network is involved (low setup cost).
#[test]
fn specification_chat_cancellation_restart_and_retry_keep_target_provenance() {
    let mut app = fixture();
    let text = "  Why is this the current product behavior?\nKeep these bytes.  ";
    let old_request = product_request(&app, text);
    let old_pin = old_request.target.published_pin.commit_sha.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let prompt = Arc::new(Mutex::new(None));
    app.task_harness = Some(Box::new(GatedSpecificationHarness {
        entered: entered_tx,
        release: release.clone(),
        prompt: prompt.clone(),
        reply: "The model tried to edit the document.".into(),
    }));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::SpecificationChat(
        old_request,
    ));
    entered_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("the production Specification command should reach its harness");

    let ctx = egui::Context::default();
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("koolade_spec_document_view"), 1_u8);
        data.insert_temp(
            egui::Id::new("koolade_selected_feature"),
            Some("F999".to_owned()),
        );
        data.insert_persisted(
            egui::Id::new("koolade_selected_feature"),
            Some("F999".to_owned()),
        );
    });
    let cancel = crate::ui::Intent {
        cancel: true,
        ..Default::default()
    };
    app.dispatch_ui_command(crate::ui::ApplicationCommand::UserIntent(cancel));
    {
        let (lock, wake) = &*release;
        *lock.lock().unwrap() = true;
        wake.notify_all();
    }
    complete(&mut app);
    let first_prompt = prompt.lock().unwrap().clone().unwrap();
    assert!(first_prompt.contains(text));
    assert!(first_prompt.contains("Scope: Product"));
    assert!(!first_prompt.contains("F999"));
    let slug = match &app.screen {
        Screen::Connected(project) => project.chat_slug.clone(),
        Screen::Welcome => panic!("fixture remains connected"),
    };
    let stored = crate::persistence::chat_store::load(&slug).0;
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    assert!(
        !project.chat.iter().any(|message| {
            message.role == crate::domain::ChatRole::Agent
                && message.text == "The model tried to edit the document."
        }),
        "the answer from the canceled turn must not appear in the live conversation"
    );
    assert!(
        !stored.iter().any(|message| {
            message.role == crate::domain::ChatRole::Agent
                && message.text == "The model tried to edit the document."
        }),
        "the answer from the canceled turn must not be persisted"
    );
    assert!(stored.iter().any(|message| {
        message.role == crate::domain::ChatRole::User
            && message.text == text
            && message
                .specification_context
                .as_ref()
                .is_some_and(|context| {
                    context.scope_key == "product"
                        && context.published_commit_sha == old_pin
                        && context.intent == "ask"
                })
    }));

    advance_product_revision(&app);
    let mut app = super::super::board_tests::restart_fixture(app);
    if let Screen::Connected(project) = &mut app.screen {
        project.chat = crate::persistence::chat_store::load(&project.chat_slug).0;
        project
            .state
            .resync()
            .expect("explicit refresh observes the newer published revision");
    }
    let new_request = product_request(&app, text);
    assert_ne!(new_request.target.published_pin.commit_sha, old_pin);
    let retry_prompts = Arc::new(Mutex::new(Vec::new()));
    app.task_harness = Some(Box::new(ReplyHarness {
        prompts: retry_prompts.clone(),
        reply: "Answer based on the current published revision.".into(),
    }));
    app.dispatch_ui_command(crate::ui::ApplicationCommand::SpecificationChat(
        new_request.clone(),
    ));
    complete(&mut app);
    let retry_prompt = retry_prompts.lock().unwrap()[0].clone();
    assert!(retry_prompt.contains(&new_request.target.published_pin.commit_sha));
    assert!(
        retry_prompt.contains(&old_pin),
        "old turn provenance stays attributed"
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let assistant = project
        .chat
        .iter()
        .rev()
        .find(|message| message.role == crate::domain::ChatRole::Agent)
        .unwrap();
    assert_eq!(
        assistant.text,
        "Answer based on the current published revision."
    );
    assert_eq!(
        assistant
            .specification_context
            .as_ref()
            .unwrap()
            .published_commit_sha,
        new_request.target.published_pin.commit_sha
    );
}

fn product_request(app: &KooladeApp, text: &str) -> crate::core::specification_chat::Request {
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    crate::core::specification_chat::Request {
        text: text.into(),
        intent: crate::core::specification_chat::Intent::Ask,
        target: crate::core::specification_chat::capture_target(
            &project.state,
            crate::core::specification_chat::Scope::Product,
        )
        .unwrap(),
        omitted_history_messages: 0,
    }
}

fn advance_product_revision(app: &KooladeApp) {
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected")
    };
    let path = project
        .state
        .repo_root
        .join("docs/koolade/product/overview.md");
    let mut content = std::fs::read_to_string(&path).unwrap();
    content.push_str("\n<!-- new published fixture revision -->\n");
    std::fs::write(&path, content).unwrap();
    for args in [
        ["add", "docs/koolade/product/overview.md"].as_slice(),
        ["commit", "-q", "-m", "advance fixture product revision"].as_slice(),
    ] {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&project.state.repo_root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

struct GatedSpecificationHarness {
    entered: mpsc::Sender<()>,
    release: Arc<(Mutex<bool>, Condvar)>,
    prompt: Arc<Mutex<Option<String>>>,
    reply: String,
}

impl AiHarness for GatedSpecificationHarness {
    fn label(&self) -> String {
        "gated Specification test harness".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        *self.prompt.lock().unwrap() = Some(request.prompt_body.clone());
        let _ = self.entered.send(());
        let (lock, wake) = &*self.release;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = wake.wait(released).unwrap();
        }
        Ok(HarnessOutcome {
            final_text: self.reply.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}
