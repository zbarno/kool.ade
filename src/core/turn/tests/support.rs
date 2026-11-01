use super::*;

pub(super) fn captured_persona(prompt: &str) -> Option<String> {
    let key = prompt.find("\"operator_persona\":")? + "\"operator_persona\":".len();
    let mut decoder = serde_json::Deserializer::from_str(prompt[key..].trim_start());
    <String as serde::Deserialize>::deserialize(&mut decoder).ok()
}

pub(super) fn vision_update(text: &str) -> Vec<DocumentUpdate> {
    vec![DocumentUpdate {
        document_id: "product:overview".into(),
        content: format!("# Overview\n\n{text}\n"),
        status: None,
    }]
}

/// Fake harness for pipeline tests: canned envelopes, zero processes.
pub(super) struct ScriptedHarness {
    pub(super) canned: Option<TurnEnvelope>,
    pub(super) raw: Option<String>,
}
impl AiHarness for ScriptedHarness {
    fn label(&self) -> String {
        "scripted-test".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("test".into())
    }
    fn execute(&self, req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        if req.mode == crate::harness::ExecutionMode::TaskGeneration {
            assert!(req.system_instructions.contains(prompt::PLANNER_POLICY));
            assert!(req.prompt_manifest.is_none());
        } else {
            assert!(
                req.system_instructions
                    .contains("Planning Analyst for Kool.ad/e")
            );
            assert_eq!(
                req.prompt_manifest
                    .as_ref()
                    .map(|manifest| manifest.role_version.as_str()),
                Some("planner_conversation/v5")
            );
        }
        let text = match (&self.raw, &self.canned) {
            (Some(raw), _) => raw.clone(),
            (_, Some(env)) => {
                let json = serde_json::to_string(env).unwrap();
                format!("Summary text here.\n\n```json\n{json}\n```")
            }
            (None, None) => "(forgetting the block entirely)".into(),
        };
        Ok(HarnessOutcome {
            final_text: text,
            envelope: None, // force the pipeline to extract + decode itself
            stderr_tail: String::new(),
        })
    }
}

pub(super) struct RetrievalPipelineHarness;

impl AiHarness for RetrievalPipelineHarness {
    fn label(&self) -> String {
        "retrieval-pipeline-test".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok(self.label())
    }

    fn plan_retrieval(&self, request: &PlanningRequest) -> Result<Option<RetrievalPlan>, AppError> {
        assert_eq!(
            request.mode,
            crate::harness::ExecutionMode::ReadOnlyAnalysis
        );
        assert!(
            request
                .system_instructions
                .contains("select authoritative context")
        );
        assert!(
            request
                .prompt_body
                .contains("The session vanishes after restart.")
        );
        Ok(Some(RetrievalPlan {
            documents: vec!["product:architecture-and-constraints".into()],
            open_items: vec![],
            repository_areas: vec!["src".into()],
        }))
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        assert_eq!(request.mode, crate::harness::ExecutionMode::Planning);
        assert!(
            request
                .system_instructions
                .contains("Planning Analyst for Kool.ad/e")
        );
        assert_eq!(
            request
                .prompt_manifest
                .as_ref()
                .map(|manifest| manifest.role_version.as_str()),
            Some("planner_conversation/v5")
        );
        assert!(
            request
                .prompt_body
                .contains("SESSION_CONTEXT_FROM_RETRIEVAL")
        );
        assert!(
            request
                .prompt_body
                .contains("Source: planning/product/architecture-and-constraints.md")
        );
        assert!(request.prompt_body.contains("repo:src/session.rs"));
        Ok(HarnessOutcome {
            final_text: serde_json::json!({
                "schema_version": 2,
                "assistant_message": "I will explain how session recovery works.",
                "change_summary": "explain session recovery"
            })
            .to_string(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

pub(super) fn head_count(dir: &std::path::Path) -> usize {
    let out = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    if !out.status.success() {
        // Unborn HEAD: the fixture repos ship uncommitted at start.
        return 0;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// Acts as a rival writer: while the model is "running" it edits the spec
/// and checkpoints, exactly as another gated turn would.
pub(super) struct RivalCheckpoint {
    pub(super) raw: String,
    pub(super) root: std::path::PathBuf,
    pub(super) path: std::path::PathBuf,
}
impl AiHarness for RivalCheckpoint {
    fn label(&self) -> String {
        "rival-checkpoint".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, _req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let text = std::fs::read_to_string(&self.path).map_err(|e| AppError::Io {
            op: "read spec".into(),
            detail: e.to_string(),
        })?;
        std::fs::write(&self.path, format!("{text}\n\n<!-- rival writer -->\n")).map_err(|e| {
            AppError::Io {
                op: "edit spec".into(),
                detail: e.to_string(),
            }
        })?;
        for args in [
            ["add", "-A"].as_slice(),
            ["commit", "-qm", "rival: external edit"].as_slice(),
        ] {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .status()
                .map_err(|e| AppError::Io {
                    op: "git".into(),
                    detail: e.to_string(),
                })?
                .success();
            assert!(ok, "rival commit failed");
        }
        Ok(HarnessOutcome {
            final_text: self.raw.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

pub(super) fn inputs_for(tag: &str, msg: &str) -> (TurnInputs, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("koolade_turn_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        ["init"].as_slice(),
        ["config", "user.email", "koolade@test.local"].as_slice(),
        ["config", "user.name", "Kool.ad/e Test"].as_slice(),
    ] {
        let _ = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output();
    }
    let mut st = PlannerState::load(&root).unwrap();
    st.bootstrap_missing().unwrap();
    let inputs = TurnInputs {
        state: st,
        user_message: msg.into(),
        recent_chat: Vec::new(),
        purpose: crate::core::workflow::TurnPurpose::Interview,
        comparison_feature: None,
    };
    let dir = inputs.state.repo_root.clone();
    (inputs, dir)
}
