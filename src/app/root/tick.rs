use super::*;

mod activity;
mod clone_job;
mod display;
mod implementations;
mod pull_requests;
mod turns;

impl KooladeApp {
    pub(super) fn tick(&mut self, _dt: f32, ctx: &egui::Context) {
        self.surface_time_ledger_errors();
        self.poll_attention(ctx);
        self.poll_setup_attention(ctx);
        self.queue_manager_setup_update();
        self.poll_clone_job();
        let (outcome, task_outcomes) = self.poll_turn_events();
        self.poll_implementations();
        self.poll_pull_requests();
        self.apply_completed_turns(outcome, task_outcomes);
        self.poll_display_refresh(ctx);
        self.advance_activity();
        self.advance_auto_publish();
        self.advance_auto_queue();
        self.advance_reconciliation();
        self.advance_investigation();
        let period = match &self.screen {
            Screen::Connected(p)
                if (p.active_turn.is_some()
                    || !p.task_turns.is_empty()
                    || !p.active_implementations.is_empty()
                    || p.reconciliation.is_running()
                    || p.investigation.is_some()
                    || p.activity.manager.is_some()) =>
            {
                Duration::from_millis(120)
            }
            _ => Duration::from_millis(800),
        };
        ctx.request_repaint_after(period);
    }
}
