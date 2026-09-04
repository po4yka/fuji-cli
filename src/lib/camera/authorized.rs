use std::time::Instant;

use binrw::{BinRead, BinWrite};

use super::{preflight::MutationPermit, ptp};
use crate::generated::cameras::CameraPreflightOperation;

/// Which validated operation may open each high-level camera path. A permit
/// carries the operation its preflight profile was selected for, so these
/// lists are what stops a session validated for one operation from driving
/// another: a simulation-read permit must never reach a write path, and a
/// recovery permit must never reach a render.
pub(super) const BACKUP_RESTORE: &[CameraPreflightOperation] =
    &[CameraPreflightOperation::BackupRestore];
pub(super) const SIMULATION_READ: &[CameraPreflightOperation] = &[
    CameraPreflightOperation::SimulationAccess,
    CameraPreflightOperation::SimulationWrite,
];
pub(super) const SIMULATION_WRITE: &[CameraPreflightOperation] =
    &[CameraPreflightOperation::SimulationWrite];
pub(super) const RENDER: &[CameraPreflightOperation] = &[CameraPreflightOperation::RawConversion];
pub(super) const RENDER_RECOVERY_FETCH: &[CameraPreflightOperation] =
    &[CameraPreflightOperation::RawRecoveryFetch];
/// A render cleans up the object it just produced, and `image recover`
/// cleans up under its own preflight, so both permits open the same path.
pub(super) const RENDER_CLEANUP: &[CameraPreflightOperation] = &[
    CameraPreflightOperation::RawConversion,
    CameraPreflightOperation::RawRecoveryCleanup,
];

fn authorize(
    permit: CameraPreflightOperation,
    allowed: &[CameraPreflightOperation],
) -> anyhow::Result<()> {
    anyhow::ensure!(
        allowed.contains(&permit),
        "validated PTP permit does not match the requested high-level operation"
    );
    Ok(())
}

pub(crate) struct AuthorizedPtp<'io> {
    ptp: &'io mut ptp::Ptp,
    permit: &'io mut MutationPermit,
}

impl<'io> AuthorizedPtp<'io> {
    pub(super) fn new(
        ptp: &'io mut ptp::Ptp,
        permit: &'io mut MutationPermit,
        allowed: &[CameraPreflightOperation],
    ) -> anyhow::Result<Self> {
        authorize(permit.operation(), allowed)?;
        Ok(Self { ptp, permit })
    }

    pub(crate) fn is_healthy(&self) -> bool {
        self.ptp.is_healthy()
    }

    pub(crate) fn mark_camera_processing_active(&mut self) {
        self.ptp.mark_camera_processing_active();
    }

    pub(crate) fn mark_camera_processing_complete(&mut self) {
        self.ptp.mark_camera_processing_complete();
    }

    pub(crate) fn send(
        &mut self,
        code: ptp::CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        self.ptp.send(code, params, data)
    }

    pub(crate) fn send_for_operation(
        &mut self,
        operation: ptp::PtpOperation,
        code: ptp::CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        self.ptp.send_for_operation(operation, code, params, data)
    }

    pub(crate) fn send_until(
        &mut self,
        deadline: Instant,
        code: ptp::CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        self.ptp.send_until(deadline, code, params, data)
    }

    pub(crate) fn send_mutating(
        &mut self,
        code: ptp::CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        self.ptp.send_mutating(self.permit, code, params, data)
    }

    pub(crate) fn send_mutating_for_operation(
        &mut self,
        operation: ptp::PtpOperation,
        code: ptp::CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        self.ptp
            .send_mutating_for_operation(self.permit, operation, code, params, data)
    }

    pub(crate) fn get_prop_raw(&mut self, code: impl Into<u16>) -> anyhow::Result<Vec<u8>> {
        self.ptp.get_prop_raw(code)
    }

    pub(crate) fn set_prop_raw(
        &mut self,
        code: impl Into<u16>,
        value: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        self.ptp.set_prop_raw(self.permit, code, value)
    }

    /// Validates a raw property value against the permit's descriptor
    /// without sending anything to the camera. Never widens the permit.
    pub(crate) fn validate_prop_raw(
        &self,
        code: impl Into<u16>,
        value: &[u8],
    ) -> anyhow::Result<()> {
        self.permit.validate_property_value(code.into(), value)
    }

    pub(crate) fn get_prop<T>(&mut self, code: impl Into<u16>) -> anyhow::Result<T>
    where
        T: for<'a> BinRead<Args<'a> = ()>,
    {
        self.ptp.get_prop(code)
    }

    pub(crate) fn set_prop<T>(&mut self, code: impl Into<u16>, value: &T) -> anyhow::Result<()>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        self.ptp.set_prop(self.permit, code, value)
    }

    pub(crate) fn set_prop_for_operation<T>(
        &mut self,
        operation: ptp::PtpOperation,
        code: impl Into<u16>,
        value: &T,
    ) -> anyhow::Result<()>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        self.ptp
            .set_prop_for_operation(self.permit, operation, code, value)
    }

    pub(crate) fn firmware_option_write_value(
        &self,
        option: &str,
        logical_value: &str,
    ) -> anyhow::Result<i32> {
        self.ptp
            .firmware_option_write_value(self.permit, option, logical_value)
    }

    pub(crate) fn firmware_capability_profile(
        &self,
    ) -> anyhow::Result<&'static crate::generated::cameras::CameraFirmwareCapabilityProfile> {
        self.ptp.firmware_capability_profile(self.permit)
    }

    pub(crate) fn firmware_option_read_logical_value(
        &self,
        option: &str,
        wire_value: i32,
    ) -> anyhow::Result<&'static str> {
        self.ptp
            .firmware_option_read_logical_value(self.permit, option, wire_value)
    }

    pub(crate) fn validate_raw_conversion_profile(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        self.ptp.validate_raw_conversion_profile(
            self.permit,
            profile_code,
            header_padding,
            fields,
            bytes,
        )
    }

    pub(crate) fn validate_raw_conversion_read_fingerprint(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        declared_field_count: u16,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        self.ptp.validate_raw_conversion_read_fingerprint(
            self.permit,
            profile_code,
            header_padding,
            declared_field_count,
            fields,
            bytes,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::generated::cameras::CameraPreflightOperation as Operation;

    use super::{
        BACKUP_RESTORE, RENDER, RENDER_CLEANUP, RENDER_RECOVERY_FETCH, SIMULATION_READ,
        SIMULATION_WRITE, authorize,
    };

    #[test]
    fn a_simulation_read_permit_opens_no_write_path() {
        authorize(Operation::SimulationAccess, SIMULATION_READ)
            .expect("reading simulations is what the access permit is for");

        for allowed in [
            SIMULATION_WRITE,
            BACKUP_RESTORE,
            RENDER,
            RENDER_RECOVERY_FETCH,
            RENDER_CLEANUP,
        ] {
            let error = authorize(Operation::SimulationAccess, allowed)
                .expect_err("an access permit must not open a state-changing path");
            assert!(error.to_string().contains("does not match"), "{error}");
        }
    }

    #[test]
    fn each_write_path_accepts_only_the_operations_it_names() {
        let cases = [
            (Operation::SimulationWrite, SIMULATION_WRITE),
            (Operation::SimulationWrite, SIMULATION_READ),
            (Operation::BackupRestore, BACKUP_RESTORE),
            (Operation::RawConversion, RENDER),
            (Operation::RawConversion, RENDER_CLEANUP),
            (Operation::RawRecoveryFetch, RENDER_RECOVERY_FETCH),
            (Operation::RawRecoveryCleanup, RENDER_CLEANUP),
        ];
        for (permit, allowed) in cases {
            authorize(permit, allowed).expect("the permit names this path");
        }

        let refused = [
            (Operation::BackupRestore, SIMULATION_WRITE),
            (Operation::RawConversion, RENDER_RECOVERY_FETCH),
            (Operation::RawRecoveryFetch, RENDER),
            (Operation::RawRecoveryFetch, RENDER_CLEANUP),
            (Operation::RawRecoveryCleanup, RENDER),
            (Operation::SimulationWrite, BACKUP_RESTORE),
        ];
        for (permit, allowed) in refused {
            authorize(permit, allowed).expect_err("the permit does not name this path");
        }
    }
}
