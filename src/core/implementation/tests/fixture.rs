use super::*;

pub(super) struct Fixture {
    pub(super) mode: &'static str,
    pub(super) calls: Arc<AtomicUsize>,
}
impl AiHarness for Fixture {
    fn label(&self) -> String {
        "implementation fixture".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, req: &PlanningRequest) -> Result<crate::harness::HarnessOutcome, AppError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(req.mode, crate::harness::ExecutionMode::Implementation);
        assert!(req.prompt_body.contains("RESUME"));
        if self.mode == "fresh_budget" {
            assert!(
                req.prompt_body
                    .contains("fresh report, verification, harness, and self-repair budget")
            );
            assert!(
                !req.prompt_body
                    .contains("Automatic correction limit and self-repair attempts exhausted")
            );
            assert!(
                !req.prompt_body
                    .contains("Correction history: Correction history:")
            );
        }
        if self.mode.starts_with("repair_") && call > 0 {
            assert!(
                req.prompt_body
                    .contains("PREVIOUS STOP / CORRECTION REQUIRED")
            );
            assert!(req.prompt_body.contains("Previous response"));
            assert_eq!(
                fs::read_to_string(req.repo_root.join("implemented.txt")).unwrap(),
                "implemented\n"
            );
            if self.mode == "repair_verification" || (self.mode == "repair_mixed" && call > 3) {
                assert!(
                    req.prompt_body
                        .contains("Verification command failed: test -f missing-file")
                );
                fs::write(
                    req.repo_root.join("missing-file"),
                    "repaired prerequisite\n",
                )
                .unwrap();
            }
            if self.mode == "repair_blocked" {
                assert!(
                    req.prompt_body
                        .contains("AUTOMATIC BLOCKER RECOVERY REQUIRED")
                );
                assert!(req.prompt_body.contains("Repair the local conductor"));
            }
            if self.mode == "repair_cancel" {
                req.cancel.store(true, Ordering::SeqCst);
            }
        }
        if self.mode == "resume" {
            assert!(
                req.prompt_body
                    .contains("PREVIOUS STOP / CORRECTION REQUIRED")
            );
            assert!(req.prompt_body.contains("Implementation cancelled"));
            assert_eq!(
                fs::read_to_string(req.repo_root.join("implemented.txt")).unwrap(),
                "partial\n"
            );
        }
        if self.mode == "cancel" {
            fs::write(req.repo_root.join("implemented.txt"), "partial\n").unwrap();
            req.cancel.store(true, Ordering::SeqCst);
        } else if self.mode != "evidence_only" {
            fs::write(req.repo_root.join("implemented.txt"), "implemented\n").unwrap();
        }
        if (self.mode == "harness_retry" && call == 0)
            || (self.mode == "sidecar" && call == 0)
            || self.mode == "harness_dead"
        {
            return Err(AppError::HarnessFailed {
                reason: "pi finished but produced no final assistant message".into(),
                stderr_tail: String::new(),
            });
        }
        if self.mode == "healing" && call >= 4 {
            assert!(req.prompt_body.contains("SELF-REPAIR REQUIRED"));
        }
        let status = if matches!(self.mode, "blocked" | "external_blocked")
            || (self.mode == "repair_blocked" && call == 0)
        {
            "blocked"
        } else {
            "complete"
        };
        let verification = if self.mode == "evidence_only" {
            "test ! -e implemented.txt"
        } else if self.mode == "fail"
            || self.mode == "repair_verification"
            || self.mode == "repair_mixed"
        {
            "test -f missing-file"
        } else {
            "test \"$(cat implemented.txt)\" = implemented"
        };
        let criterion = if self.mode == "evidence_only" {
            "Repository remains unchanged."
        } else {
            "File contains implemented."
        };
        let blocker_disposition = if status == "blocked" {
            if self.mode == "external_blocked" {
                "human_action"
            } else {
                "machine_repair"
            }
        } else {
            "none"
        };
        let mut report = serde_json::json!({"schemaVersion":1,"status":status,"blocker_disposition":blocker_disposition,"summary":"Implemented the ticket behavior.","acceptance_criteria":[{"criterion":criterion,"evidence":"Created the required evidence and checked its exact contents."}],"verification":[verification],"remaining":[]});
        if status == "blocked" {
            report["remaining"] = if self.mode == "external_blocked" {
                serde_json::json!(["Adjudicator: approve the revised contract before resuming."])
            } else {
                serde_json::json!(["Repair the local conductor and rerun the acceptance check."])
            };
        }
        if call == 0 && self.mode == "repair_criterion" {
            report["acceptance_criteria"][0]["criterion"] = "File contains implementation.".into();
        }
        let final_text = if (self.mode == "healing" && call < 4)
            || (self.mode == "repair_mixed" && call < 3)
            || (call == 0 && matches!(self.mode, "repair_markdown" | "repair_cancel"))
        {
            "# Summary\nImplemented the ticket.".into()
        } else if call == 0 && self.mode == "repair_schema" {
            "{\"status\":\"complete\"}".into()
        } else {
            report.to_string()
        };
        Ok(crate::harness::HarnessOutcome {
            final_text,
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}
