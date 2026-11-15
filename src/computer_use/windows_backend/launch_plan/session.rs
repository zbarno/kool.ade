use std::path::Path;

use super::{WindowsLaunchBlocker, WindowsWpxLaunchInput, WindowsWpxLaunchPlan, path};

impl WindowsWpxLaunchPlan {
    pub(crate) fn from_authorized_session(
        session: &crate::computer_use::ComputerSessionV1,
        overlay: &crate::computer_use::guest_image::GuestImageOverlayV1,
        storage: &super::super::storage::WindowsVmStorage,
        qemu_executable: &Path,
        boot_config: &Path,
        host_logical_processors: u16,
    ) -> Result<Self, WindowsLaunchBlocker> {
        if session.session_class != crate::computer_use::ComputerSessionClassV1::Disposable {
            return Err(WindowsLaunchBlocker::UnsupportedSessionClass);
        }
        if session.network_profile != crate::computer_use::ComputerNetworkProfileV1::Disabled
            || session.policy.network_profile
                != crate::computer_use::ComputerNetworkProfileV1::Disabled
        {
            return Err(WindowsLaunchBlocker::NetworkNotDisabled);
        }
        if session.policy.validate().is_err()
            || !path::canonical_session_id(&session.identity.session_id)
        {
            return Err(WindowsLaunchBlocker::InvalidResourceBudget);
        }
        let session_directory = storage
            .session_directory(&session.identity.session_id)
            .map_err(|_| WindowsLaunchBlocker::UnsafePath)?;
        let guest_control_socket = storage
            .guest_control_socket_path(&session.identity.session_id)
            .map_err(|_| WindowsLaunchBlocker::UnsafePath)?;
        let budget = &session.policy.resource_budget;
        let plan = Self::build(
            WindowsWpxLaunchInput {
                qemu_executable,
                overlay: overlay.path(),
                overlay_root: storage.overlay_root(),
                overlay_virtual_size_bytes: overlay.virtual_size_bytes(),
                boot_config,
                session_directory: &session_directory,
                guest_control_socket: &guest_control_socket,
                session_id: &session.identity.session_id,
                cpu_millis: budget.cpu_millis,
                memory_bytes: budget.memory_bytes,
                disk_bytes: budget.disk_bytes,
                disposable_session: session.session_class
                    == crate::computer_use::ComputerSessionClassV1::Disposable,
                network_disabled: session.network_profile
                    == crate::computer_use::ComputerNetworkProfileV1::Disabled
                    && session.policy.network_profile
                        == crate::computer_use::ComputerNetworkProfileV1::Disabled,
            },
            host_logical_processors,
        )?;
        for path in [qemu_executable, overlay.path(), boot_config] {
            path::validate_native_path_no_reparse(path)?;
        }
        let overlay_root = std::fs::canonicalize(storage.overlay_root())
            .map_err(|_| WindowsLaunchBlocker::UnsafePath)?;
        let canonical_overlay =
            std::fs::canonicalize(overlay.path()).map_err(|_| WindowsLaunchBlocker::UnsafePath)?;
        let session_root = std::fs::canonicalize(&session_directory)
            .map_err(|_| WindowsLaunchBlocker::UnsafePath)?;
        let canonical_boot =
            std::fs::canonicalize(boot_config).map_err(|_| WindowsLaunchBlocker::UnsafePath)?;
        if canonical_overlay.parent() != Some(overlay_root.as_path())
            || canonical_boot.parent() != Some(session_root.as_path())
        {
            return Err(WindowsLaunchBlocker::UnsafePath);
        }
        Ok(plan)
    }
}
