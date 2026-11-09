//! Curated plain-English explanations for the static machine-setup catalog.
//! Deterministic by construction: no network, no provider, no dynamic facts.
//! Each entry is a const tuple so copy-on-use costs nothing.

/// Curated copy for one catalog task.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub changes: &'static [&'static str],
    pub why: &'static str,
    pub security: &'static str,
    pub reversibility: &'static str,
    pub rollback: Option<&'static str>,
    pub declined: &'static str,
}

type RowType = (
    &'static str,
    &'static [&'static str],
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
);

const OPEN_DECLINE: &str = "Setup keeps working around this step; dependent items remain visibly open. Each operation is still offered individually with its own scoped approval, so you can decide again later.";
/// The full curated table, one row per static catalog task.
pub(crate) const ENTRIES: &[RowType] = &[
    (
        "setup:bootstrap",
        &[
            "Creates Kool.ad/e's local working folders (spec, workspace, and setup ledger) under its own data directory.",
        ],
        "Every later step assumes these exist and are writable.",
        "Created files stay inside the Kool.ad/e data directory.",
        "Files can be removed manually later.",
        Some(
            "Deleting the Kool.ad/e data directory resets local state; remote specs are unaffected.",
        ),
        OPEN_DECLINE,
    ),
    (
        "setup:docker:install",
        &[
            "Installs Docker Engine for Windows using the official Microsoft Store channel, or the platform-standard installer on macOS/Linux.",
        ],
        "Containers host the isolated runtimes that specs depend on.",
        "Official distribution channel only; no mirror lists or community builds.",
        "Undoable.",
        Some("Uninstalling Docker removes the engine and leaves volumes intact."),
        OPEN_DECLINE,
    ),
    (
        "setup:docker:desktop",
        &["Links Kool.ad/e to the already-installed Docker Desktop product."],
        "Desktop bundles engine + UI + extensions in one supportable surface.",
        "No installation happens; only the connection is probed.",
        "Disconnect-only.",
        None,
        "Engine steps that assume a live socket keep failing until the link is restored.",
    ),
    (
        "setup:docker:daemon",
        &["Restarts the local Docker daemon service once (admin consent requested)."],
        "Configuration changes only take effect on restart; a wedged daemon blocks everything else.",
        "Restart touches only the Docker service; other services are unaffected.",
        "Transitional.",
        None,
        "Stale configuration stays in effect; later steps may misdiagnose themselves.",
    ),
    (
        "setup:docker:start",
        &["Starts the stopped Docker service."],
        "A present-but-stopped engine makes every container check fail upstream.",
        "Starting a standard local service; no ports opened automatically.",
        "Reversible.",
        None,
        "Container-dependent steps stay blocked.",
    ),
    (
        "setup:docker:permission",
        &["Adds the local service-account relationship Docker needs to reach its socket."],
        "Without socket permission, the engine is invisible to Kool.ad/e despite running.",
        "Scoped ACL/service entry for the Docker socket only.",
        "Undoable.",
        Some("Removing the grant restores the prior access boundary."),
        OPEN_DECLINE,
    ),
    (
        "setup:docker:workspace",
        &["Shares this machine's project folder with the container runtime as a mount point."],
        "Workspace mounts are how containers read spec artifacts without copying them.",
        "Mount shares the stated path read/write with containers; narrower scopes are not possible for mounted work.",
        "Removable.",
        Some("Unmounting and removing the share restores the prior file exposure."),
        "Container workloads that read the workspace stall; host access is unchanged.",
    ),
    (
        "setup:docker:disk",
        &["Expands or defrags the WSL2 virtual disk holding container storage (Windows)."],
        "A packed virtual disk makes pulls fail with confusing allocation errors.",
        "Disk-level maintenance scoped to the WSL2 vhdx; host partitions untouched.",
        "Data-preserving.",
        None,
        "Storage pressure symptoms return at the next heavy pull.",
    ),
    (
        "setup:docker:rootless",
        &[
            "Switches the Docker daemon configuration to the user namespace and verifies a working non-root socket.",
        ],
        "Rootless containment shrinks blast radius if a container ever escapes.",
        "Privilege reduction only; no elevated grants are taken.",
        "Config revert.",
        Some("Toggling the setting back to rootful restores the prior mode."),
        "Hardened containment stays unavailable; the risk note stays open.",
    ),
    (
        "setup:docker:image-architecture",
        &["Retags or re-verifies the local image to match this host's CPU architecture."],
        "An architecture mismatch fails at boot time, weeks from the cause.",
        "Touches only the image store metadata for the named image.",
        "Re-taggable.",
        None,
        "Boot-time architecture failures would surface later.",
    ),
    (
        "setup:docker:image-digest",
        &["Replaces floating-tagged critical images with digest-pinned copies."],
        "Digest pinning guarantees the runtime bytes nobody can silently swap.",
        "Download restricted to the pinned digest from the approved registry.",
        "Image store entry swap.",
        Some("Tagging the previous digest back restores it."),
        "Floating tags keep drifting; supply-chain assurance gap remains.",
    ),
    (
        "setup:docker:image-unverified",
        &[
            "Boots the unverified image once in an ephemeral container to confirm the runtime starts.",
        ],
        "Catch packaging faults now, not mid-runbook.",
        "Throwaway container; no persistent state written outside the image cache.",
        "Ephemeral.",
        None,
        "Broken runtime discovered mid-task later.",
    ),
    (
        "setup:docker:os",
        &[
            "Applies the small set of OS prerequisites the engine's release notes demand (kernel modules, services, feature flags).",
        ],
        "Missing kernel features surface as baffling runtime failures.",
        "Standard package-service installs from the platform vendor; feature toggles limited to what the release notes cite.",
        "Mostly reversible.",
        Some("Feature flags flip back; packages uninstall cleanly."),
        "Latent runtime failures stay latent.",
    ),
    (
        "setup:docker:api",
        &["Runs a read-only REST call to the local Docker API (version + capabilities handshake)."],
        "Everything above ultimately talks to this API; checking it directly localizes faults and proves connectivity in one step.",
        "Probe traffic is loopback-only.",
        "Pure probe.",
        None,
        "Fault localization stays imprecise.",
    ),
    (
        "setup:docker:capabilities",
        &["Queries the engine for advertised features (buildkit, cgroup driver, seccomp support)."],
        "Capability gaps explain build failures far faster than trial and error.",
        "Loopback query; no mutations.",
        "Pure query.",
        None,
        OPEN_DECLINE,
    ),
    (
        "setup:docker:architecture",
        &["Records the host and Docker CPU architectures so later image picks are compatible."],
        "Many portability mistakes start from assuming both sides are x86-64.",
        "Read-only introspection.",
        "Pure query.",
        None,
        OPEN_DECLINE,
    ),
    (
        "setup:docker:endpoint",
        &[
            "Verifies the reachable engine endpoint (socket path or TCP URI) responds and records it for other steps.",
        ],
        "Endpoint drift is a common 'nothing works' root cause.",
        "Connectivity probe only; the recorded value is used verbatim by later steps.",
        "Recording is deletable.",
        None,
        "Other steps keep hitting stale endpoints.",
    ),
    (
        "setup:docker:profile",
        &["Selects and confirms the active daemon/runtime profile required by this workspace."],
        "Profiles isolate resource limits per workload class.",
        "Selection switches the active profile; no resources granted beyond what the profile defines.",
        "Switchable.",
        None,
        "Workloads requiring the profile defer.",
    ),
    (
        "setup:docker:limits",
        &["Applies the memory / CPU ceiling the workspace demands to the daemon config."],
        "Unchecked ceilings let one runaway container starve the desktop.",
        "Limits constrain containers, not the host OS.",
        "Editable afterwards.",
        None,
        "Starvation risk stays theoretical.",
    ),
    (
        "setup:docker:seccomp",
        &[
            "Ensures default seccomp filtering is enabled; disables it only when a verified requirement justifies (documented).",
        ],
        "Seccomp is a core kernel-layer defense; weakening it is exceptional.",
        "Weakening affects new containers only; the change is disclosed in the approval.",
        "Re-enableable.",
        Some("Saving the custom profile allows restoring stock behaviour."),
        "Weakens containment; the exception note stays open.",
    ),
    (
        "setup:docker:cgroups",
        &["Aligns the cgroup driver / v2 hierarchy with the runtime's expectations."],
        "Mixed cgroup setups deadlock systemd-managed containers.",
        "Hierarchy alignment touches cgroup bookkeeping, not process state.",
        "Redoable.",
        None,
        "Deadlock-prone configurations persist.",
    ),
    (
        "setup:docker:windows-containers",
        &["Enables the Windows-container subsystem alongside Linux ones (optional dual mode)."],
        "Dual-mode widens compatibility at modest complexity cost.",
        "Subsystem enablement via standard OS feature toggles.",
        "Toggle-off.",
        None,
        "Windows-container workloads stay unsupported.",
    ),
    (
        "setup:linux:rootless:packages",
        &[
            "Installs the user-namespace package set (uidmap/newuidmap, slirp4netns) from the distro.",
        ],
        "Rootless mode cannot start without setuid helpers.",
        "Distro-packaged binaries only. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Package-manager revert.",
        Some("Removing the packages restores the prior state."),
        OPEN_DECLINE,
    ),
    (
        "setup:linux:rootless:subids",
        &["Registers subordinate UID/GID ranges for the current user (subuid/subgid entries)."],
        "Namespace maps need allocated ranges; the system reserves them here.",
        "Entries scoped to the invoking user; other users untouched.",
        "Removable.",
        Some("Deleting the subuid/subgid lines restores the prior map space."),
        "Rootless mode keeps failing on namespace allocation.",
    ),
    (
        "setup:linux:rootless:service",
        &["Installs and starts the per-user rootless Docker service (systemd user unit)."],
        "A stopped user service mimics a missing engine to every check.",
        "Service runs as YOUR user, not root — privilege floor drops, not rises.",
        "Stoppable/removable unit.",
        None,
        OPEN_DECLINE,
    ),
    (
        "setup:computer:qemu",
        &[
            "Installs the QEMU user-static binaries used for cross-architecture container emulation.",
        ],
        "Cross-arch builds are common; binfmt makes transparent emulation work.",
        "Binary registration only executes emulator for foreign images on demand.",
        "Uninstallable.",
        None,
        "Foreign-arch containers would hang at exec.",
    ),
    (
        "setup:computer:kvm",
        &["Loads/enables the KVM module group (nested virt where the hypervisor permits)."],
        "Accelerated VMs are dramatically faster than emulated for dev containers.",
        "Kernel module activation; nested flag only where firmware exposes it. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Module unload.",
        Some("Unloading the modules restores the prior schedule."),
        OPEN_DECLINE,
    ),
    (
        "setup:computer:storage",
        &["Checks free space and allocates the volume area Kool.ad/e manages."],
        "Low-disk failures corrupt caches mid-operation if unchecked.",
        "Allocation stays inside the designated data volume. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Volume removable.",
        Some("Deleting the volume frees the space."),
        OPEN_DECLINE,
    ),
    (
        "setup:computer:network",
        &["Verifies baseline internet reachability and DNS resolution used by downloads."],
        "Diagnoses whether later download failures are ours or the link's.",
        "Probes only; certificates validated as usual. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Read-only.",
        None,
        OPEN_DECLINE,
    ),
    (
        "setup:computer:personal-encryption",
        &["Reports disk encryption status; recommends enabling if absent."],
        "Encrypted disks protect specs and cached credentials at rest.",
        "Reporting only; Kool.ad/e never toggles device encryption by itself. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Informational.",
        None,
        "Recommendation stands until acted upon.",
    ),
    (
        "setup:computer:viewer",
        &["Captures a redacted screenshot of the display for support context (opt-in)."],
        "Support turnaround improves sharply with visual context.",
        "Capture passes through the redactor that strips obvious credential-shaped glyphs; you approve scope first. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Screenshot skippable/deletable.",
        None,
        OPEN_DECLINE,
    ),
    (
        "setup:git:install",
        &["Installs Git from the platform-standard channel."],
        "Every planning artifact and every repo interaction presumes git.",
        "Standard vendor channels only.",
        "Uninstallable.",
        None,
        "Repo features dormant.",
    ),
    (
        "setup:git:identity",
        &[
            "Writes a LOCAL repository-scope user.name/user.email if unset (values you supply in the dialog, never guessed).",
        ],
        "Commits without identity fail or mint ghost authors.",
        "Local scope only; global dotfiles untouched. You type the values yourself. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Config lines editable/removable.",
        None,
        "Commit attribution problems surface per repo.",
    ),
    (
        "setup:git:verify",
        &["Smoke-tests git client + server round trip on the configured remotes."],
        "Auth expiries and cert issues hide until pushed.",
        "Round trip limited to auth negotiation and ls-remote style reads. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Read-only.",
        None,
        OPEN_DECLINE,
    ),
    (
        "setup:host:unsupported",
        &[
            "No automatic action — documents what would be required and provides manual instructions.",
        ],
        "Honesty beats fake-green: unsupported platforms get a paper trail.",
        "Nothing executed on the host. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "N/A.",
        None,
        "Item remains informational.",
    ),
    (
        "setup:windows:wsl-features",
        &[
            "Enables the Windows components WSL2 requires (Virtual Machine Platform, WSL), then requests reboot.",
        ],
        "WSL2 cannot initialize without these feature bits.",
        "Standard DISM feature enables; no payload downloaded beyond signed Microsoft components. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Features can be disabled (data caveat).",
        Some("Disablement keeps the WSL distro store."),
        "Container parity stays Windows-emulated or absent.",
    ),
    (
        "setup:windows:virtualization",
        &["Detects hardware-virt exposure; explains BIOS/UEFI enablement steps where hidden."],
        "Nested hypervisors frequently mask VT-x from the guest.",
        "Detection + guidance only; firmware untouched. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Informational.",
        None,
        OPEN_DECLINE,
    ),
    (
        "setup:windows:host-version",
        &["Compares the OS build against the minimum WSL2-supported build."],
        "Old builds fail WSL2 in ways updates trivially fix.",
        "Version comparison only. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Informational.",
        None,
        "Upgrade guidance remains manual.",
    ),
    (
        "setup:windows:reboot",
        &[
            "Schedules a graceful sign-out-safe reboot window after enabling virtualization features.",
        ],
        "Several WSL2 feature bits only bind at boot.",
        "Reboot initiated through the normal shutdown path with warning. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Deferrable indefinitely.",
        None,
        "Bound features keep waiting; nothing else is forced.",
    ),
    (
        "setup:windows:wsl-kernel",
        &["Updates the WSL2 Linux kernel package to the current Microsoft-signed build."],
        "Kernel lag produces obscure cgroup/net namespaces bugs.",
        "Microsoft-signed kernel binary only; no third-party tree. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Previous kernel selectable.",
        None,
        OPEN_DECLINE,
    ),
    (
        "setup:windows:wsl-policy",
        &[
            "Audits enterprise/group policies that can neuter WSL (device guard, applocker-style restrictions) and names the offending policy, not a workaround.",
        ],
        "Corporate policy silently overriding local fixes is the classic blind spot.",
        "Policy registry read-only; Kool.ad/e never edits policy objects. Passwords, tokens, or verification codes are never entered for you, and any secret material produced stays inside the named credential store.",
        "Advisory only.",
        None,
        "Restrictions persist; so does the notification.",
    ),
];

/// Static helper so rows above can compose the shared secret-handling clause
/// while keeping a `&'static str` type.
/// Resolve a curated entry by task id.
pub fn lookup(id: &str) -> Option<Entry> {
    ENTRIES.iter().find(|(rid, ..)| *rid == id).copied().map(
        |(_, changes, why, security, rev, rb, declined)| Entry {
            changes,
            why,
            security,
            reversibility: rev,
            rollback: rb,
            declined,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curated_entry_carries_complete_copy_no_placeholders() {
        assert_eq!(ENTRIES.len(), 41);
        for (id, changes, why, security, reversibility, _, declined) in ENTRIES {
            assert!(!why.is_empty(), "{id}");
            assert!(!security.is_empty(), "{id}");
            assert!(!reversibility.is_empty(), "{id}");
            assert!(!declined.is_empty(), "{id}");
            assert!(!why.contains("__PLACEHOLDER__"), "{id}");
            assert!(changes.len() <= 4, "{id}");
            for c in *changes {
                assert!(!c.is_empty(), "{id}");
            }
            // The honest security floor: copy may describe handling but must
            // never promise to solicit credentials.
            assert!(
                !security.to_lowercase().contains("enter your password"),
                "{id}"
            );
        }
    }

    #[test]
    fn no_two_entries_share_an_id() {
        let mut seen = std::collections::HashSet::new();
        for (id, ..) in ENTRIES {
            assert!(seen.insert(id), "duplicate {id}");
        }
    }

    #[test]
    fn lookup_hits_core_rows() {
        for id in [
            "setup:bootstrap",
            "setup:docker:rootless",
            "setup:windows:wsl-features",
            "setup:computer:viewer",
        ] {
            assert!(lookup(id).is_some(), "{id}");
        }
        assert!(lookup("setup:nope:nowhere").is_none());
    }
}
