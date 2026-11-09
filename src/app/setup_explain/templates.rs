//! Vetted parametric templates for families whose IDs carry structured
//! tokens (coding-assistant provider stages, locked runtime images).
//! Tokens come exclusively from parsing the task ID Kool.ad/e produced;
//! nothing is invented or fetched remotely.

use crate::persistence::setup_tasks::SetupTaskV1;

#[derive(Clone, Debug)]
pub struct Rendered {
    pub headline: String,
    pub what_changes: Vec<String>,
    pub why: String,
    pub security: String,
    pub reversibility: String,
    pub rollback_note: Option<String>,
    pub if_declined: String,
}

impl Rendered {
    fn new() -> Self {
        Self {
            headline: String::new(),
            what_changes: Vec::new(),
            why: String::new(),
            security: String::new(),
            reversibility: String::new(),
            rollback_note: None,
            if_declined: String::new(),
        }
    }

    fn finish(mut self) -> Self {
        if self.security.is_empty() {
            self.security =
                "Network access is limited to the involved party's own services. Kool.ad/e \
                 never requests passwords, API keys, or security codes in chat."
                    .to_string();
        }
        if self.if_declined.is_empty() {
            self.if_declined = "Nothing breaks today; the affected checks stay listed and you \
                                can revisit this step anytime."
                .to_string();
        }
        self
    }
}

fn friendly(name: &str) -> String {
    let pretty = name.replace('-', " ");
    format!("“{pretty}”")
}

fn provider_stage(provider: &str, stage: &str) -> Rendered {
    let who = format!("{} coding assistant", friendly(provider));
    let mut r = Rendered::new();
    match stage {
        "version" | "path" | "installed" => {
            r.headline = format!("Locate the {who} command line");
            r.what_changes = vec![
                "Nothing is installed or edited; the check only looks for the installed command \
                 and reports its version."
                    .to_string(),
            ];
            r.why = format!(
                "Later setup and delegated steps hand work to {who}; it must be found \
                        first."
            );
            r.reversibility = "Read-only check.".to_string();
            r.if_declined = format!(
                "Delegated steps that assume {who} stay unavailable until you install it \
                 yourself and re-run setup checks."
            );
        }
        "install" => {
            r.headline = format!("Install the {who} command line");
            r.what_changes = vec![
                "Downloads the official program for this platform from the vendor's canonical \
                     distribution location and places it on your PATH. The vendor's own \
                     configuration lives in your home directory."
                    .to_string(),
            ];
            r.why = format!("Setup and delegated steps expect {who} to work from a regular shell.");
            r.security = "Only the vendor's canonical distribution is used, and credentials \
                          are never sent during installation."
                .to_string();
            r.reversibility = "Undoable.".to_string();
            r.rollback_note = Some(
                "Uninstalling the vendor's command and deleting its home-directory settings \
                     restores the prior state."
                    .to_string(),
            );
            r.if_declined =
                format!("Steps that delegate to {who} stay unavailable; everything else proceeds.");
        }
        "authenticate" | "auth" | "auth-verify" => {
            r.headline = format!("Sign in with {who}");
            r.what_changes = vec![
                "Opens the vendor's own sign-in flow in your browser. You enter your \
                 credentials directly at the vendor — Kool.ad/e never sees, stores, or \
                 forwards them."
                    .to_string(),
            ];
            r.why = format!(
                "Signed-in access is required for steps that call on {who}. Vendor sessions \
                 expire, so this may reasonably recur."
            );
            r.security = "Credentials go to the vendor only. Kool.ad/e learns success or \
                          failure, never the password, token, or code itself."
                .to_string();
            r.reversibility =
                "Signing out at the vendor ends the session whenever you want.".to_string();
            r.if_declined = format!(
                "Steps that delegate to {who} stay disabled until you sign in \
                        yourself."
            );
        }
        "models" | "plans" => {
            r.headline = format!("Ask {who} what it offers");
            r.what_changes = vec![
                "Makes one read-only query of the vendor's account endpoint for available plans \
                 and models; no local files are touched."
                    .to_string(),
            ];
            r.why = format!("Knowing what {who} offers lets setup suggest sensible defaults.");
            r.reversibility = "Read-only network query.".to_string();
            r.if_declined = "Planning advice stays generic until this succeeds.".to_string();
        }
        "capability" | "feature" | "flag" | "experimental" | "verify" => {
            r.headline = format!("Sanity-check {who}");
            r.what_changes = vec![
                "Runs a throwaway verification command confined to the scratchpad area; \
                 nothing else on the machine is written or changed."
                    .to_string(),
            ];
            r.why = "Version drift can break delegated steps quietly; this catches it.".to_string();
            r.reversibility = "Scratch-area activity only.".to_string();
            r.if_declined =
                format!("Drift in {who} goes unnoticed and delegated steps may fail later.");
        }
        "doctor" | "diagnose" | "troubleshoot" => {
            r.headline = format!("Run {who}'s bundled diagnostic");
            r.what_changes = vec![
                "Runs the vendor's own diagnostic; a summarized output is kept locally."
                    .to_string(),
            ];
            r.why = "Pinpointing degradation early keeps later repairs smaller.".to_string();
            r.reversibility = "A diagnostic run; the log entry can be discarded.".to_string();
            r.if_declined = "The diagnosis stays pending.".to_string();
        }
        _ => {
            r.headline = format!("Verify a {who} capability (“{stage}”)");
            r.what_changes = vec![format!(
                "Performs the “{stage}” verification that this check describes, strictly \
                 inside the permissions listed in the upcoming approval preview."
            )];
            r.why = format!("Confirms {who} is dependable before delegated steps rely on it.");
            r.reversibility = "The approval preview before execution spells out exactly what is \
                 touched."
                .to_string();
            r.if_declined = "Dependent checks for this provider remain open.".to_string();
        }
    }
    r.finish()
}

fn locked_image(profile: &str, arch: &str, digest: &str) -> Rendered {
    let hex_part = digest.strip_prefix("sha256:").unwrap_or(digest);
    let preview = if hex_part.len() > 20 {
        format!(
            "sha256:{}…{}",
            &hex_part[..12],
            &hex_part[hex_part.len() - 8..]
        )
    } else {
        format!("sha256:{hex_part}")
    };
    let profile_nice = friendly(profile);
    let mut r = Rendered::new();
    r.headline = format!("Pull the pinned {profile_nice} container image ({arch})");
    r.what_changes = vec![format!(
        "Downloads this exact image — {preview} — from the pinned catalog into the local \
         Docker image cache, then starts a throwaway container to confirm the runtime \
         boots. Already-cached copies are used as-is."
    )];
    r.why = format!(
        "Container work for {profile_nice} environments must start from a known-good, \
         digest-pinned base rather than a floating tag anyone could repoint."
    );
    r.security = "The digest pins the exact bytes: a substituted or corrupted image cannot \
                  pass. No account data leaves the machine."
        .to_string();
    r.reversibility = "Safe to reverse.".to_string();
    r.rollback_note =
        Some("Deleting the image from the Docker cache restores the prior state.".to_string());
    r.if_declined = format!(
        "Work that depends on the {profile_nice} image waits; the rest of setup continues."
    );
    r.finish()
}

pub fn render(task: &SetupTaskV1) -> Option<Rendered> {
    let parts: Vec<&str> = task.id.split(':').collect();
    let rest = parts.get(1..)?.to_vec();
    if rest.first() == Some(&"cli") && rest.len() == 3 {
        return Some(provider_stage(rest[1], rest[2]));
    }
    if rest.first() == Some(&"images") && rest.len() == 4 {
        let safe_profile = rest[1].bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
        if safe_profile {
            return Some(locked_image(rest[1], rest[2], rest[3]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::SetupExplanation;
    use crate::persistence::setup_tasks::State;

    fn task(id: &str) -> crate::persistence::setup_tasks::SetupTaskV1 {
        crate::persistence::setup_tasks::SetupTaskV1 {
            schema_version: 1,
            id: id.into(),
            title: id.into(),
            explanation: String::new(),
            next_step: String::new(),
            dependencies: Vec::new(),
            blocked_by: Vec::new(),
            state: State::Detected,
            created_at_unix_ms: 1,
            observed_at_unix_ms: 1,
            scan_generation: 1,
            verified_at_unix_ms: None,
            verified_result: None,
            attempts: Vec::new(),
            events: Vec::new(),
            superseded_by: Vec::new(),
        }
    }

    #[test]
    fn provider_stages_render_parametric_templates_not_raw_tokens() {
        for id in [
            "setup:cli:codex:authenticate",
            "setup:cli:opencode:unknown-stage",
            "setup:cli:pi:install",
        ] {
            let rendered = SetupExplanation::explain(&task(id));
            assert!(matches!(rendered, SetupExplanation::Parametric(_)), "{id}");
            let text = format!(
                "{} {:?} {}",
                rendered.headline(),
                rendered.what_changes(),
                rendered.if_declined()
            );
            assert!(!text.contains("{{"), "unrendered brace in {id}: {text}");
        }
    }

    #[test]
    fn image_digest_preview_is_truncated_not_printed_whole_in_headline() {
        let id =
            "setup:images:general:x86_64:0123456789abcdef0123456789abcdef0123456789abcdef01234567";
        let rendered = SetupExplanation::explain(&task(id));
        assert!(matches!(rendered, SetupExplanation::Parametric(_)));
        assert!(!rendered.headline().contains(id), "full digest in headline");
        assert!(rendered.what_changes()[0].contains("0123456789ab"));
    }
}
