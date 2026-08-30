use std::time::Instant;

use binrw::{BinRead, BinWrite};

use super::{preflight::MutationPermit, ptp};

pub(crate) struct AuthorizedPtp<'io> {
    ptp: &'io mut ptp::Ptp,
    permit: &'io mut MutationPermit,
}

impl<'io> AuthorizedPtp<'io> {
    pub(super) fn new(
        ptp: &'io mut ptp::Ptp,
        permit: &'io mut MutationPermit,
        allowed: &[crate::generated::cameras::CameraPreflightOperation],
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            allowed.contains(&permit.operation()),
            "validated PTP permit does not match the requested high-level operation"
        );
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
