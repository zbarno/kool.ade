use super::*;

// F-16 in-app pi harness setup guide (D-15: rendered-only, in this card)
// ---------------------------------------------------------------------------

/// Type alias so the dialog layer names the harness's display-purpose
/// snapshot without repeating the path.
pub type ProbeReport = crate::harness::pi_harness::ProbeReport;

/// Live discovery state painted in the guide: a per-open background probe,
/// pending until it replies.
#[derive(Clone, Default, Debug, PartialEq)]
pub enum ProbeView {
    /// The probe thread has not replied yet — the card shows the pending
    /// hint and NO detail line.
    #[default]
    Pending,
    /// The detached probe delivered its report.
    Report(ProbeReport),
}

/// Styling class for one rendered guide line (pins the golden-text tests).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GuideLineKind {
    Title,
    Lead,
    Status,
    Detail,
    Order,
    Rule,
    Step,
}

/// One rendered line of the guide. Lines compose PURELY so the NFR-8
/// golden-text pins can exercise the copy without painting widgets.
#[derive(Debug, PartialEq)]
pub struct GuideLine {
    pub kind: GuideLineKind,
    pub text: String,
}

/// Compose the full "Set up the pi harness" guide for the given live
/// state. Pure: identical inputs yield identical lines. The discovery copy
/// is SINGLE-SOURCED from the harness (`PI_BINARY_ENV`, `COMMON_HOME_SITES`),
/// so the rendered order can never drift from the executed order; when
/// `home` is `None` (HOME unset) the order lines print literal `$HOME`,
/// mirroring `locate_binary` skipping its home sites entirely.
pub fn harness_guide_lines(view: &ProbeView, home: Option<&str>) -> Vec<GuideLine> {
    let env = crate::harness::pi_harness::PI_BINARY_ENV;
    let home_display = home.unwrap_or("$HOME");
    let mut lines = Vec::new();
    lines.push(GuideLine {
        kind: GuideLineKind::Title,
        text: "Set up the pi harness".into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Lead,
        text: "Kool.ad/e shells out to a locally installed pi CLI; it downloads and \
               installs nothing itself."
            .into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Status,
        text: match view {
            ProbeView::Pending => "Looking for the pi CLI…".to_string(),
            ProbeView::Report(r) => r.status.clone(),
        },
    });
    if let ProbeView::Report(r) = view {
        // Pending intentionally shows NO detail line: the hint stands alone
        // until the probe actually says something.
        lines.push(GuideLine {
            kind: GuideLineKind::Detail,
            text: if r.ok {
                format!(
                    "Winning binary: {}",
                    r.binary
                        .as_deref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                )
            } else {
                r.diagnostic.clone()
            },
        });
    }
    lines.push(GuideLine {
        kind: GuideLineKind::Rule,
        text: "Discovery order — first match wins:".into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Order,
        text: format!(
            "1. {env} override: if set and executable it wins outright; \
             a bad value fails fast with no fall-through."
        ),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Order,
        text: "2. pi in every PATH directory, in PATH order.".into(),
    });
    for (i, site) in crate::harness::pi_harness::COMMON_HOME_SITES
        .iter()
        .enumerate()
    {
        lines.push(GuideLine {
            kind: GuideLineKind::Order,
            text: format!("{}. {home_display}/{site}/pi", i + 3),
        });
    }
    lines.push(GuideLine {
        kind: GuideLineKind::Rule,
        text: "Version policy: no floor, no pinning — any installed pi is \
              accepted; the probed version is display-only (D-13)."
            .into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Lead,
        text: "Install & make discoverable:".into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Step,
        text: "1. Obtain pi via the vendor channel — npm install -g \
               @earendil-works/pi-coding-agent (adjust if the vendor's \
               documented channel differs)."
            .into(),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Step,
        text: format!(
            "2. Make it reachable via PATH, a home location above, or {env}=/\
             abs/path/to/pi in the launching environment."
        ),
    });
    lines.push(GuideLine {
        kind: GuideLineKind::Step,
        text: "3. Reopen this dialog and confirm the status reads 'pi \
              <version>'."
            .into(),
    });
    lines
}

// ---------------------------------------------------------------------------
/// Drain whatever the detached probe has queued since the last frame; flip
/// `dlg.probe_view` to the newest report and forget a channel whose sender
/// has gone (probe delivered, or died with its superseded open). Borrow-only
/// phase first (the receiver cannot be cloned), then commit the mutations.
pub fn drain_probe(dlg: &mut DlgSettings) {
    let (incoming, sender_gone) = match dlg.probe_rx.as_ref() {
        Some(rx) => {
            let mut incoming = None;
            let mut gone = false;
            loop {
                match rx.recv_timeout(std::time::Duration::ZERO) {
                    Ok(rep) => incoming = Some(rep),
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        gone = true;
                        break;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                }
            }
            (incoming, gone)
        }
        None => (None, false),
    };
    if let Some(rep) = incoming {
        dlg.probe_view = ProbeView::Report(rep);
    }
    if sender_gone {
        dlg.probe_rx = None;
    }
}
