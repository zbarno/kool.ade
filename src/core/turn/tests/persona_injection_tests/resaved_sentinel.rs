use super::*;

#[test]
fn resaved_sentinel_reaches_the_second_controller_without_cache_or_restart() {
    run_leg(|attempt, obs| {
        // Both turns live under ONE claimed home: turn one stores
        // sentinel A, then B is stored IN-PROCESS — no relaunch,
        // no restart — and turn two under a FRESH controller must
        // carry it.
        let home = LockedPersonaHome::new(&format!("nocache_{attempt}"));
        save_persona(SENTINEL_A).expect("sentinel A save must succeed");
        if !bound_here(&home) {
            obs.push(format!(
                "nocache leg: pre-flight env check lost at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        let (inputs, dir_a) = inputs_for("pj_nocache_a", "First pass, tuned voice.");
        let sink_a: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink_a) = controller_for(inputs, &sink_a, "First pass acknowledged.");
        let (outcome, _acts) = drain_collecting(&c);
        drop(c);
        if !bound_here(&home) {
            obs.push(format!(
                "nocache leg: env claim lost mid-turn one at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir_a);
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("turn one must Apply: {outcome:?}"));
        }
        let first = match sink_a.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("turn one captured no instructions".into()),
        };
        let _ = std::fs::remove_dir_all(&dir_a);

        save_persona(SENTINEL_B).expect("sentinel B save must succeed");
        if !bound_here(&home) {
            obs.push(format!(
                "nocache leg: env check lost between re-save and turn two at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }

        let (inputs, dir_b) = inputs_for("pj_nocache_b", "Second pass, retuned voice.");
        let sink_b: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink_b) = controller_for(inputs, &sink_b, "Second pass acknowledged.");
        let (outcome, _acts) = drain_collecting(&c);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir_b);
        if !bound_here(&home) {
            obs.push(format!(
                "nocache leg: env claim lost mid-turn two at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("turn two must Apply: {outcome:?}"));
        }
        let second = match sink_b.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("turn two captured no instructions".into()),
        };
        let first_persona = captured_persona(&first);
        let second_persona = captured_persona(&second);
        if !(first_persona.as_deref() == Some(SENTINEL_A)
            && second_persona.as_deref() == Some(SENTINEL_B)
            && first != second)
        {
            obs.push(format!(
                "nocache leg unbound at attempt {attempt}: first_A={}, second_B={}",
                first_persona.as_deref() == Some(SENTINEL_A),
                second_persona.as_deref() == Some(SENTINEL_B)
            ));
            return LegResult::Crossing;
        }
        if first_persona == second_persona {
            return LegResult::Fatal(
                "persona changes must be visible in the next user prompt".into(),
            );
        }
        LegResult::Bound
    });
}
