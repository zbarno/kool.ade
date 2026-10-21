use super::*;

#[test]
fn unservable_persona_falls_back_to_shipped_default_with_surfaced_diagnostic() {
    // Leg 1: persona.md overwritten with NON-UTF-8 bytes (a 0xFF
    // then 0xFE header plus junk).
    run_leg(|attempt, obs| {
        let (inputs, dir) = inputs_for("pj_corrupt", "How does the SSO draft stand?");
        let home = LockedPersonaHome::new(&format!("fb_corrupt_{attempt}"));
        let corrupt: &[u8] = &[0xFF, 0xFE, b'j', b'u', b'n', b'k'];
        std::fs::write(persona_path(), corrupt).expect("write the corrupt bytes");
        if !bound_here(&home) {
            obs.push(format!(
                "corrupt leg: pre-flight env check lost at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir);
            return LegResult::Crossing;
        }
        let sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink) = controller_for(inputs, &sink, "Standing reported; nothing to decide yet.");
        let (outcome, acts) = drain_collecting(&c);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
        if !bound_here(&home) {
            obs.push(format!(
                "corrupt leg: env claim lost mid-turn at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!(
                "corrupt-file leg must still APPLY (never a harness failure): {outcome:?}"
            ));
        }
        let instr = match sink.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("corrupt leg captured no instructions".into()),
        };
        if !instr.contains("Planning Analyst for Kool.ad/e") {
            return LegResult::Fatal("the versioned planning role must remain present".into());
        }
        let path = persona_path().to_string_lossy().to_string();
        // The store diagnostic travels VERBATIM through the
        // notification: full message frame anchored on this
        // home's path.
        let verbatim_noticed = acts.iter().any(|a| {
            a.starts_with("Persona note: persona file ")
                && a.contains(&path)
                && a.ends_with(" is not valid UTF-8; serving the shipped default")
        });
        if !(captured_persona(&instr).as_deref() == Some(SHIPPED_DEFAULT_PERSONA)
            && verbatim_noticed)
        {
            obs.push(format!(
                "corrupt leg unbound at attempt {attempt}: default_loaded={}, acts={:?}",
                captured_persona(&instr).as_deref() == Some(SHIPPED_DEFAULT_PERSONA),
                acts
            ));
            return LegResult::Crossing;
        }
        LegResult::Bound
    });

    // Leg 2: persona.md deleted outright (presence established
    // first so the removal is the operative state).
    run_leg(|attempt, obs| {
        let (inputs, dir) = inputs_for("pj_deleted", "Same question, please.");
        let home = LockedPersonaHome::new(&format!("fb_deleted_{attempt}"));
        save_persona("# doomed draft\n").expect("setup save must succeed");
        std::fs::remove_file(persona_path()).expect("delete-leg setup");
        assert!(!persona_path().exists(), "delete-leg precondition");
        if !bound_here(&home) {
            obs.push(format!(
                "delete leg: pre-flight env check lost at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir);
            return LegResult::Crossing;
        }
        let sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink) = controller_for(inputs, &sink, "Same standing; still nothing to decide.");
        let (outcome, acts) = drain_collecting(&c);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
        if !bound_here(&home) {
            obs.push(format!(
                "delete leg: env claim lost mid-turn at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("delete leg must still APPLY: {outcome:?}"));
        }
        let instr = match sink.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("delete leg captured no instructions".into()),
        };
        let path = persona_path().to_string_lossy().to_string();
        // Same verbatim bar for the seed-path diagnostic.
        let noticed = acts.iter().any(|a| {
            a.starts_with("Persona note: persona file ")
                && a.contains(&path)
                && a.ends_with(" was absent; seeded the shipped default")
        });
        if !(captured_persona(&instr).as_deref() == Some(SHIPPED_DEFAULT_PERSONA) && noticed) {
            obs.push(format!(
                "delete leg unbound at attempt {attempt}: default_loaded={}, acts={:?}",
                captured_persona(&instr).as_deref() == Some(SHIPPED_DEFAULT_PERSONA),
                acts
            ));
            return LegResult::Crossing;
        }
        LegResult::Bound
    });
}
