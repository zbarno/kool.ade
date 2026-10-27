use crate::app::bootstrap_repair_policy::Operation;

use super::super::{
    Cancellation, ElevationReceipt, HostIdentity, NativeElevation, ProcessRunFailure,
    RemediationPlan, SystemCommandRunner, TrustedInvocation,
};

pub(crate) struct LinuxNativeElevation;

impl NativeElevation for LinuxNativeElevation {
    fn request(
        &self,
        plan: &RemediationPlan,
        identity: &HostIdentity,
        invocation: &TrustedInvocation,
        cancellation: &Cancellation,
    ) -> anyhow::Result<Option<ElevationReceipt>> {
        anyhow::ensure!(
            matches!(
                plan.operation,
                Operation::InstallLinuxRootlessPackages | Operation::ConfigureLinuxSubordinateIds
            ) && plan.host == crate::app::bootstrap_repair_policy::Host::Linux,
            "native elevation is not available for this remediation operation"
        );
        if cancellation.is_cancelled() {
            return Ok(None);
        }
        crate::app::bootstrap_remediation::validate_before_elevated_launch(invocation)?;
        let preflight = invocation.native_elevation_preflight()?;
        match SystemCommandRunner.run_elevation_preflight(preflight, cancellation) {
            Ok(output) if output.exit_code == 0 => Ok(Some(
                ElevationReceipt::issued_after_native_prompt(plan, identity, invocation)?,
            )),
            Ok(_) => Ok(None),
            Err(error) if error.failure == ProcessRunFailure::Cancelled => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}
