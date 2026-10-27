use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, ProbeObservationV1 as Observation,
};

use super::super::{ProbeFact, ProbeRead, fact};

const MIN_SUBID_RANGE: u64 = 65_536;

pub(super) fn inspect(read: ProbeRead<String>, uid: u32, username: Option<&str>) -> ProbeFact {
    let (state, code) = match read {
        ProbeRead::Found(contents) => {
            let state = parse_subid_file(&contents, uid, username);
            let code = match state {
                State::Ready => Code::LinuxHostCheckReady,
                State::Missing => Code::LinuxHostCheckMissing,
                State::Unknown => Code::HostCapabilityNotProbed,
                State::Failed => Code::LinuxProbeMalformed,
                State::Unsupported => Code::LinuxProbeUnsupported,
                _ => Code::HostCapabilityNotProbed,
            };
            (state, code)
        }
        // A missing account database file is not equivalent to an empty file:
        // the current usermod flow validates existing root-owned files and
        // cannot safely initialize absent system files itself.
        ProbeRead::Missing => (State::Unknown, Code::HostCapabilityNotProbed),
        ProbeRead::Inaccessible => (State::Unknown, Code::LinuxProbeInaccessible),
        ProbeRead::Malformed => (State::Failed, Code::LinuxProbeMalformed),
    };
    fact(
        state,
        code,
        Some(Observation::LinuxSubordinateIds {
            configured: state == State::Ready,
        }),
    )
}

fn parse_subid_file(contents: &str, uid: u32, username: Option<&str>) -> State {
    let numeric_uid = uid.to_string();
    let mut ambiguous_owner = false;
    let mut insufficient = false;
    for line in contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if line.starts_with('#') {
            continue;
        }
        let mut fields = line.split(':');
        let Some(owner) = fields.next() else {
            continue;
        };
        let owner_matches = owner == numeric_uid || username == Some(owner);
        let owner_might_match = username.is_none() && owner.parse::<u32>().is_err();
        let Some(start_text) = fields.next() else {
            if owner_matches {
                return State::Failed;
            }
            if owner_might_match {
                ambiguous_owner = true;
            }
            continue;
        };
        let Some(count_text) = fields.next() else {
            if owner_matches {
                return State::Failed;
            }
            if owner_might_match {
                ambiguous_owner = true;
            }
            continue;
        };
        if fields.next().is_some() {
            if owner_matches {
                return State::Failed;
            }
            if owner_might_match {
                ambiguous_owner = true;
            }
            continue;
        }
        if !owner_matches {
            if owner_might_match {
                ambiguous_owner = true;
            }
            continue;
        }
        let (Ok(start), Ok(count)) = (start_text.parse::<u64>(), count_text.parse::<u64>()) else {
            return State::Failed;
        };
        if start == 0 || start <= u64::from(uid) || count == 0 || start.checked_add(count).is_none()
        {
            return State::Failed;
        }
        if count >= MIN_SUBID_RANGE {
            return State::Ready;
        }
        insufficient = true;
    }
    if ambiguous_owner {
        State::Unknown
    } else if insufficient {
        // Existing short mappings need an operator decision. The automated
        // usermod operation only adds a full range when this account has no
        // prior entries, so do not advertise a repair that will be rejected.
        State::Unknown
    } else {
        State::Missing
    }
}
