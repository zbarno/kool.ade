use super::*;

#[test]
fn saved_sentinel_rides_both_conversation_modes_from_the_next_turn_on() {
    // Main-Chat leg.
    run_leg(|attempt, obs| {
        let (inputs, dir_main) = inputs_for("pj_main", "Speak in my tuned voice.");
        let home = LockedPersonaHome::new(&format!("modes_main_{attempt}"));
        save_persona(SENTINEL_A).expect("sentinel save must succeed");
        if !bound_here(&home) {
            obs.push(format!(
                "main leg: pre-flight env check lost at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir_main);
            return LegResult::Crossing;
        }
        let sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink) = controller_for(inputs, &sink, "Noted; answering in your tuned voice.");
        let (outcome, acts) = drain_collecting(&c);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir_main);
        if !bound_here(&home) {
            obs.push(format!(
                "main leg: env claim lost mid-turn at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("main-chat leg must Apply: {outcome:?}"));
        }
        let main_instr = match sink.lock().unwrap().take() {
            Some(i) => i,
            None => {
                return LegResult::Fatal("main-chat turn captured no instructions".into());
            }
        };
        let persona = captured_persona(&main_instr);
        if persona.as_deref() != Some(SENTINEL_A)
            || !main_instr.contains("Planning Analyst for Kool.ad/e")
            || main_instr.contains(SENTINEL_A)
        {
            obs.push(format!(
                "main leg unbound at attempt {attempt}: persona_matches={}, acts={:?}",
                persona.as_deref() == Some(SENTINEL_A),
                acts
            ));
            return LegResult::Crossing;
        }
        // Negative of the diagnostic channel is a flat test
        // property (FATAL, not an env clue): a healthy load
        // emits no `Persona note:` activity. Disqualifying only
        // note-shaped activities keeps future benign mid-turn
        // notices from masquerading as a home crossing and
        // burning the redo budget.
        if !acts.iter().all(|a| !a.starts_with("Persona note:")) {
            return LegResult::Fatal(format!(
                "healthy load unexpectedly surfaced a diagnostic note: {acts:?}"
            ));
        }
        if !main_instr.contains("PRODUCT INTENT INTERVIEW")
            || main_instr.contains("TASK CONVERSATION MODE:")
        {
            return LegResult::Fatal(format!(
                "main-mode topology drifted: interview={}, task_banner={}, user={}",
                main_instr.contains("PRODUCT INTENT INTERVIEW"),
                main_instr.contains("TASK CONVERSATION MODE:"),
                main_instr.chars().take(1800).collect::<String>()
            ));
        }
        LegResult::Bound
    });

    // Scoped task-conversation leg: the SAME stored sentinel
    // must ride the scoped mode too, with the banner indexing
    // strictly before the layer.
    run_leg(|attempt, obs| {
        let (mut inputs, dir_task) = inputs_for("pj_task", "Which SSO route do you recommend?");
        let home = LockedPersonaHome::new(&format!("modes_task_{attempt}"));
        save_persona(SENTINEL_A).expect("sentinel save must succeed");
        if !bound_here(&home) {
            obs.push(format!(
                "task leg: pre-flight env check lost at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir_task);
            return LegResult::Crossing;
        }
        inputs.state.items.push(crate::domain::OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::Normal,
            crate::domain::ItemKind::Question,
            "Engineering".into(),
            None,
            "Pick the SSO route.".into(),
            "Rollout blocks on it.".into(),
        ));
        let key = inputs
            .state
            .items
            .last()
            .expect("CLR-001 pushed")
            .conversation_key()
            .to_string();
        let sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let scoped = TurnController::start_scoped(
            inputs,
            Box::new(CaptureHarness {
                raw: canned_raw("Recommendation: the least-friction route."),
                sink: Arc::clone(&sink),
            }),
            Some(key),
        );
        let (outcome, _acts) = drain_collecting(&scoped);
        drop(scoped);
        let _ = std::fs::remove_dir_all(&dir_task);
        if !bound_here(&home) {
            obs.push(format!(
                "task leg: env claim lost mid-turn at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("scoped task leg must Apply: {outcome:?}"));
        }
        let task_instr = match sink.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("scoped turn captured no instructions".into()),
        };
        let persona = captured_persona(&task_instr);
        if persona.as_deref() != Some(SENTINEL_A)
            || !task_instr.contains("TASK CONVERSATION MODE")
            || task_instr.contains(SENTINEL_A)
        {
            obs.push(format!(
                "task leg unbound at attempt {attempt}: persona_matches={}",
                persona.as_deref() == Some(SENTINEL_A)
            ));
            return LegResult::Crossing;
        }
        if task_instr.contains("PRODUCT INTENT INTERVIEW") {
            return LegResult::Fatal("task mode must omit the main-mode interview section".into());
        }
        LegResult::Bound
    });
}
