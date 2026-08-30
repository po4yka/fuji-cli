mod chunk_policy;
pub mod codec;
pub mod container;
mod descriptor;
pub mod error;
pub mod option;
pub mod props;
pub mod structs;

pub use container::*;
pub use props::*;
pub use structs::*;

pub(crate) use chunk_policy::ChunkPolicy;
pub(crate) use descriptor::*;

use std::{
    cell::Cell,
    cmp::min,
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::Cursor,
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow, bail, ensure};
use binrw::{BinRead, BinWrite, Endian};
use log::{debug, error, trace, warn};
use rusb::GlobalContext;

const PTP_BULK_TIMEOUT: Duration = Duration::from_secs(10);
const MIN_PTP_BULK_TIMEOUT: Duration = Duration::from_millis(1);
const PTP_COMMAND_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const PTP_DATA_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const PTP_STANDARD_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const PTP_CAMERA_PROCESSING_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const PTP_LARGE_TRANSFER_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const PTP_TRANSACTION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const PTP_LARGE_TRANSFER_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub const MAX_PTP_CONTAINER_PAYLOAD_BYTES: usize = 128 * 1024 * 1024;
const MAX_PTP_BULK_READ_CHUNK_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PtpOperation {
    Standard,
    LargeTransfer,
    CameraProcessing,
    Polling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PtpDeadlinePhase {
    Transaction,
    CommandWrite,
    DataTransfer,
    Response,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PtpDeadlineKind {
    Idle,
    Hard,
}

#[derive(Debug)]
struct PtpDeadlineExceeded {
    phase: PtpDeadlinePhase,
    kind: PtpDeadlineKind,
}

impl fmt::Display for PtpDeadlineExceeded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let phase = match self.phase {
            PtpDeadlinePhase::Transaction => "transaction",
            PtpDeadlinePhase::CommandWrite => "command-write",
            PtpDeadlinePhase::DataTransfer => "data-transfer",
            PtpDeadlinePhase::Response => "response",
        };
        let kind = match self.kind {
            PtpDeadlineKind::Idle => "idle",
            PtpDeadlineKind::Hard => "hard",
        };
        write!(
            formatter,
            "PTP transaction deadline exceeded (phase={phase}, kind={kind})"
        )
    }
}

impl std::error::Error for PtpDeadlineExceeded {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PtpTimeoutPolicy {
    command_idle_timeout: Duration,
    data_idle_timeout: Duration,
    response_timeout: Duration,
    transaction_timeout: Duration,
}

impl PtpOperation {
    const fn timeout_policy(self) -> PtpTimeoutPolicy {
        match self {
            Self::Standard => PtpTimeoutPolicy {
                command_idle_timeout: PTP_COMMAND_IDLE_TIMEOUT,
                data_idle_timeout: PTP_DATA_IDLE_TIMEOUT,
                response_timeout: PTP_STANDARD_RESPONSE_TIMEOUT,
                transaction_timeout: PTP_TRANSACTION_TIMEOUT,
            },
            Self::LargeTransfer => PtpTimeoutPolicy {
                command_idle_timeout: PTP_COMMAND_IDLE_TIMEOUT,
                data_idle_timeout: PTP_DATA_IDLE_TIMEOUT,
                response_timeout: PTP_LARGE_TRANSFER_RESPONSE_TIMEOUT,
                transaction_timeout: PTP_LARGE_TRANSFER_TIMEOUT,
            },
            Self::CameraProcessing => PtpTimeoutPolicy {
                command_idle_timeout: PTP_COMMAND_IDLE_TIMEOUT,
                data_idle_timeout: PTP_DATA_IDLE_TIMEOUT,
                response_timeout: PTP_CAMERA_PROCESSING_RESPONSE_TIMEOUT,
                transaction_timeout: PTP_TRANSACTION_TIMEOUT,
            },
            Self::Polling => PtpTimeoutPolicy {
                command_idle_timeout: PTP_COMMAND_IDLE_TIMEOUT,
                data_idle_timeout: PTP_DATA_IDLE_TIMEOUT,
                response_timeout: PTP_LARGE_TRANSFER_RESPONSE_TIMEOUT,
                transaction_timeout: PTP_LARGE_TRANSFER_TIMEOUT,
            },
        }
    }
}

trait BulkTransport {
    fn read_bulk(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> rusb::Result<usize>;

    fn write_bulk(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> rusb::Result<usize>;
}

trait Clock {
    fn now(&self) -> Instant;
}

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

struct Deadline<'a, C> {
    clock: &'a C,
    expires_at: Cell<Instant>,
    hard_expires_at: Instant,
    progress_timeout: Option<Duration>,
    phase: PtpDeadlinePhase,
}

pub(crate) struct BulkReadState {
    buffer: Vec<u8>,
    cursor: usize,
    len: usize,
    previous_read_filled_buffer: bool,
}

impl<'a, C: Clock> Deadline<'a, C> {
    fn new(clock: &'a C, timeout: Duration) -> anyhow::Result<Self> {
        let expires_at = clock
            .now()
            .checked_add(timeout)
            .ok_or_else(|| anyhow!("PTP transaction deadline overflow"))?;
        Ok(Self {
            clock,
            expires_at: Cell::new(expires_at),
            hard_expires_at: expires_at,
            progress_timeout: None,
            phase: PtpDeadlinePhase::Transaction,
        })
    }

    fn until(clock: &'a C, expires_at: Instant) -> anyhow::Result<Self> {
        Ok(Self {
            clock,
            expires_at: Cell::new(expires_at),
            hard_expires_at: expires_at,
            progress_timeout: None,
            phase: PtpDeadlinePhase::Transaction,
        })
    }

    fn with_idle_timeout(
        clock: &'a C,
        hard_expires_at: Instant,
        idle_timeout: Duration,
    ) -> anyhow::Result<Self> {
        let expires_at = clock
            .now()
            .checked_add(idle_timeout)
            .ok_or_else(|| anyhow!("PTP transfer idle deadline overflow"))?;
        Ok(Self {
            clock,
            expires_at: Cell::new(min(expires_at, hard_expires_at)),
            hard_expires_at,
            progress_timeout: Some(idle_timeout),
            phase: PtpDeadlinePhase::DataTransfer,
        })
    }

    fn record_progress(&self) -> anyhow::Result<()> {
        let Some(progress_timeout) = self.progress_timeout else {
            return Ok(());
        };
        let expires_at = self
            .clock
            .now()
            .checked_add(progress_timeout)
            .ok_or_else(|| anyhow!("PTP transfer idle deadline overflow"))?;
        self.expires_at.set(min(expires_at, self.hard_expires_at));
        Ok(())
    }

    fn phase(&self, phase: PtpDeadlinePhase, idle_timeout: Duration) -> anyhow::Result<Self> {
        let mut deadline = Self::with_idle_timeout(self.clock, self.hard_expires_at, idle_timeout)?;
        deadline.phase = phase;
        Ok(deadline)
    }

    fn io_timeout(&self) -> anyhow::Result<Duration> {
        let now = self.clock.now();
        let hard_remaining = self.hard_expires_at.saturating_duration_since(now);
        if hard_remaining < MIN_PTP_BULK_TIMEOUT {
            return Err(PtpDeadlineExceeded {
                phase: self.phase,
                kind: PtpDeadlineKind::Hard,
            }
            .into());
        }
        let remaining = self.expires_at.get().saturating_duration_since(now);
        if remaining < MIN_PTP_BULK_TIMEOUT {
            return Err(PtpDeadlineExceeded {
                phase: self.phase,
                kind: PtpDeadlineKind::Idle,
            }
            .into());
        }
        Ok(min(PTP_BULK_TIMEOUT, remaining))
    }
}

impl BulkReadState {
    pub(crate) fn new(chunk_size: usize) -> anyhow::Result<Self> {
        ensure!(
            chunk_size >= ContainerInfo::SIZE,
            "PTP chunk size must fit the container header"
        );
        ensure!(
            chunk_size <= MAX_PTP_BULK_READ_CHUNK_BYTES,
            "PTP chunk size {chunk_size} exceeds maximum bulk read allocation {MAX_PTP_BULK_READ_CHUNK_BYTES}"
        );
        let mut buffer = Vec::new();
        reserve_bytes(&mut buffer, chunk_size, "PTP bulk read chunk")?;
        buffer.resize(chunk_size, 0);

        Ok(Self {
            buffer,
            cursor: 0,
            len: 0,
            previous_read_filled_buffer: false,
        })
    }

    fn resized(&self, chunk_size: usize) -> anyhow::Result<Self> {
        ensure!(
            !self.has_pending_bytes(),
            "cannot resize PTP bulk read buffer with pending bytes"
        );
        let mut resized = Self::new(chunk_size)?;
        resized.previous_read_filled_buffer = self.previous_read_filled_buffer;
        Ok(resized)
    }

    fn read_exact<T: BulkTransport, C: Clock>(
        &mut self,
        transport: &T,
        bulk_in: u8,
        output: &mut [u8],
        deadline: &Deadline<'_, C>,
        truncated_message: &str,
    ) -> anyhow::Result<()> {
        let mut written = 0;
        while written < output.len() {
            if self.cursor == self.len {
                let n = loop {
                    let n = match transport.read_bulk(
                        bulk_in,
                        &mut self.buffer,
                        deadline.io_timeout()?,
                    ) {
                        Ok(n) => n,
                        Err(rusb::Error::Timeout) => continue,
                        Err(error) => return Err(error.into()),
                    };
                    deadline.record_progress()?;
                    if n != 0 {
                        break n;
                    }
                    ensure!(self.previous_read_filled_buffer, "{truncated_message}");
                    self.previous_read_filled_buffer = false;
                };
                ensure!(
                    n <= self.buffer.len(),
                    "PTP bulk read exceeded its requested length"
                );
                self.cursor = 0;
                self.len = n;
                self.previous_read_filled_buffer = n == self.buffer.len();
            }

            let available = self.len - self.cursor;
            let needed = output.len() - written;
            let copied = min(available, needed);
            output[written..written + copied]
                .copy_from_slice(&self.buffer[self.cursor..self.cursor + copied]);
            self.cursor += copied;
            written += copied;
        }
        Ok(())
    }

    fn has_pending_bytes(&self) -> bool {
        self.cursor < self.len
    }
}

impl BulkTransport for rusb::DeviceHandle<GlobalContext> {
    fn read_bulk(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> rusb::Result<usize> {
        Self::read_bulk(self, endpoint, buf, timeout)
    }

    fn write_bulk(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> rusb::Result<usize> {
        Self::write_bulk(self, endpoint, buf, timeout)
    }
}

pub struct Ptp {
    pub(crate) bus: u8,
    pub(crate) address: u8,
    pub(crate) interface: u8,
    pub(crate) bulk_in: u8,
    pub(crate) bulk_out: u8,
    pub(crate) handle: rusb::DeviceHandle<GlobalContext>,
    pub(crate) transaction_id: u32,
    pub(crate) chunk_policy: ChunkPolicy,
    pub(crate) bulk_read_state: BulkReadState,
    pub(crate) poisoned: bool,
    pub(crate) camera_processing_active: bool,
    pub(crate) mutation_authorization: Option<MutationAuthorization>,
}

#[derive(Debug)]
pub(crate) struct MutationAuthorization {
    operations: BTreeSet<u16>,
    properties: BTreeMap<u16, DevicePropDesc>,
    capability_profile: &'static crate::generated::cameras::CameraFirmwareCapabilityProfile,
    raw_conversion_read_fingerprint_validated: bool,
    raw_conversion_profile_validated: bool,
}

impl MutationAuthorization {
    fn new(
        operations: &[u16],
        properties: Vec<DevicePropDesc>,
        capability_profile: &'static crate::generated::cameras::CameraFirmwareCapabilityProfile,
    ) -> Self {
        Self {
            operations: operations.iter().copied().collect(),
            properties: properties
                .into_iter()
                .map(|descriptor| (descriptor.property_code, descriptor))
                .collect(),
            capability_profile,
            raw_conversion_read_fingerprint_validated: false,
            raw_conversion_profile_validated: false,
        }
    }

    fn validate(
        &self,
        code: CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        let operation = u16::from(code);
        ensure!(
            self.operations.contains(&operation),
            "PTP mutation 0x{operation:04x} is not authorized by the validated preflight profile"
        );
        if matches!(
            code,
            CommandCode::FujiSendObjectInfo | CommandCode::FujiSendObject
        ) {
            ensure!(
                self.raw_conversion_profile_validated,
                "RAW image upload requires a validated firmware conversion profile"
            );
        }
        if code == CommandCode::SetDevicePropValue {
            let property = params
                .first()
                .and_then(|value| u16::try_from(*value).ok())
                .ok_or_else(|| anyhow!("SetDevicePropValue requires one u16 property code"))?;
            self.validate_property_candidate(
                property,
                data.ok_or_else(|| anyhow!("SetDevicePropValue requires serialized data"))?,
            )?;
        }
        Ok(())
    }

    fn validate_property_candidate(&self, property: u16, data: &[u8]) -> anyhow::Result<()> {
        let descriptor = self.properties.get(&property).ok_or_else(|| {
            anyhow!("PTP property 0x{property:04x} was not validated by preflight")
        })?;
        descriptor.validate_serialized_candidate(data)
    }

    fn validate_raw_conversion_profile(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        self.capability_profile.validate_raw_conversion_signature(
            profile_code,
            header_padding,
            fields,
            bytes.len(),
        )?;
        ensure!(
            self.raw_conversion_read_fingerprint_validated,
            "RAW conversion write requires a matching live read fingerprint"
        );
        self.validate_property_candidate(
            u16::from(DevicePropCode::FujiRawConversionProfile),
            bytes,
        )?;
        self.raw_conversion_profile_validated = true;
        Ok(())
    }

    fn validate_raw_conversion_read_fingerprint(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        declared_field_count: u16,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        self.capability_profile
            .validate_raw_conversion_read_fingerprint(
                profile_code,
                header_padding,
                declared_field_count,
                fields,
                bytes.len(),
            )?;
        validate_raw_conversion_live_envelope(
            bytes,
            profile_code,
            header_padding,
            declared_field_count,
            fields.len(),
        )?;
        self.raw_conversion_read_fingerprint_validated = true;
        Ok(())
    }
}

impl Ptp {
    pub(crate) fn is_healthy(&self) -> bool {
        session_is_safe_to_close(
            self.poisoned,
            self.transaction_id,
            self.camera_processing_active,
        )
    }

    pub(crate) fn mark_camera_processing_active(&mut self) {
        self.camera_processing_active = true;
    }

    pub(crate) fn mark_camera_processing_complete(&mut self) {
        self.camera_processing_active = false;
    }

    pub(crate) fn send(
        &mut self,
        code: CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        self.send_for_operation(PtpOperation::Standard, code, params, data)
    }

    pub(crate) fn send_for_operation(
        &mut self,
        operation: PtpOperation,
        code: CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        self.validate_mutation(code, params, data)?;
        let started = Instant::now();
        let read_chunk_size = self.chunk_policy.read.effective_bytes;
        let write_chunk_size = self.chunk_policy.write.effective_bytes;
        let result = send_with_transport_for_operation(
            &self.handle,
            self.bulk_in,
            self.bulk_out,
            write_chunk_size,
            &mut self.bulk_read_state,
            &mut self.transaction_id,
            &mut self.poisoned,
            code,
            params,
            data,
            operation,
        );
        self.finish_transport_transaction(code, read_chunk_size, write_chunk_size, started, result)
    }

    pub(crate) fn send_until(
        &mut self,
        deadline: Instant,
        code: CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        self.validate_mutation(code, params, data)?;
        let started = Instant::now();
        let read_chunk_size = self.chunk_policy.read.effective_bytes;
        let write_chunk_size = self.chunk_policy.write.effective_bytes;
        let result = send_with_transport_until_and_read_state(
            &self.handle,
            self.bulk_in,
            self.bulk_out,
            write_chunk_size,
            &mut self.bulk_read_state,
            &mut self.transaction_id,
            &mut self.poisoned,
            code,
            params,
            data,
            deadline,
        );
        self.finish_transport_transaction(code, read_chunk_size, write_chunk_size, started, result)
    }

    fn finish_transport_transaction(
        &mut self,
        code: CommandCode,
        read_chunk_size: usize,
        write_chunk_size: usize,
        started: Instant,
        result: anyhow::Result<Vec<u8>>,
    ) -> anyhow::Result<Vec<u8>> {
        let elapsed = started.elapsed();
        match result {
            Ok(response) => {
                trace!(
                    "PTP transport complete: code={code:?}, outcome=ok, response_bytes={}, read_chunk_bytes={read_chunk_size}, write_chunk_bytes={write_chunk_size}, elapsed_ms={}",
                    response.len(),
                    elapsed.as_millis()
                );
                if is_read_only_command(code) && !self.bulk_read_state.has_pending_bytes() {
                    self.observe_read_success(response.len(), elapsed);
                }
                Ok(response)
            }
            Err(error) => {
                trace!(
                    "PTP transport complete: code={code:?}, outcome=error, response_bytes=0, read_chunk_bytes={read_chunk_size}, write_chunk_bytes={write_chunk_size}, elapsed_ms={}",
                    elapsed.as_millis()
                );
                Err(error)
            }
        }
    }

    fn observe_read_success(&mut self, response_bytes: usize, elapsed: Duration) {
        let Some(promotion) = self
            .chunk_policy
            .observe_read_only_success(response_bytes, elapsed)
        else {
            return;
        };

        match self.bulk_read_state.resized(promotion.new_bytes) {
            Ok(read_state) => {
                self.bulk_read_state = read_state;
                debug!(
                    "Promoted PTP read chunk: old_bytes={}, new_bytes={}, sample_bytes={}, sample_elapsed_ms={}",
                    promotion.old_bytes,
                    promotion.new_bytes,
                    promotion.sample_bytes,
                    promotion.sample_duration.as_millis()
                );
            }
            Err(error) => {
                self.chunk_policy.read.effective_bytes = promotion.old_bytes;
                warn!(
                    "Keeping previous PTP read chunk after promotion allocation failed: old_bytes={}, attempted_bytes={}, error={error}",
                    promotion.old_bytes, promotion.new_bytes
                );
            }
        }
    }

    pub(crate) fn open_session(&mut self, session_id: u32) -> anyhow::Result<()> {
        debug!("Opening PTP session");
        self.send(CommandCode::OpenSession, &[session_id], None)?;
        Ok(())
    }

    pub(crate) fn close_session(&mut self, _: u32) -> anyhow::Result<()> {
        debug!("Closing PTP session");
        self.send(CommandCode::CloseSession, &[], None)?;
        Ok(())
    }

    pub(crate) fn close_session_until(&mut self, deadline: Instant) -> anyhow::Result<()> {
        debug!("Closing PTP session");
        self.send_until(deadline, CommandCode::CloseSession, &[], None)?;
        Ok(())
    }

    pub(crate) fn get_info(&mut self) -> anyhow::Result<DeviceInfo> {
        debug!("Retrieving device info");
        let response = self.send(CommandCode::GetDeviceInfo, &[], None)?;
        let info = codec::decode_exact(&response)?;
        Ok(info)
    }

    pub(crate) fn get_prop_raw(&mut self, prop: impl Into<u16>) -> anyhow::Result<Vec<u8>> {
        let prop = prop.into();
        debug!("Getting device prop: 0x{prop:04x}");
        let response = self.send(CommandCode::GetDevicePropValue, &[u32::from(prop)], None)?;
        Ok(response)
    }

    pub(crate) fn get_prop_desc(&mut self, prop: impl Into<u16>) -> anyhow::Result<DevicePropDesc> {
        let prop = prop.into();
        let response = self.get_prop_desc_raw(prop)?;
        let descriptor = DevicePropDesc::decode(&response)
            .with_context(|| format!("decoding PTP device prop descriptor 0x{prop:04x}"))?;
        ensure!(
            descriptor.property_code == prop,
            "PTP device property descriptor code mismatch: requested 0x{prop:04x}, received 0x{:04x}",
            descriptor.property_code
        );
        Ok(descriptor)
    }

    pub(crate) fn get_prop_desc_raw(&mut self, prop: impl Into<u16>) -> anyhow::Result<Vec<u8>> {
        let prop = prop.into();
        debug!("Getting device prop descriptor: 0x{prop:04x}");
        self.send(CommandCode::GetDevicePropDesc, &[u32::from(prop)], None)
    }

    pub(crate) fn set_prop_raw(
        &mut self,
        prop: impl Into<u16>,
        value: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        let prop = prop.into();
        debug!("Setting device prop: 0x{prop:04x}");
        let response = self.send(
            CommandCode::SetDevicePropValue,
            &[u32::from(prop)],
            Some(value),
        )?;
        Ok(response)
    }

    pub(crate) fn get_prop<T>(&mut self, code: impl Into<u16>) -> anyhow::Result<T>
    where
        T: for<'a> BinRead<Args<'a> = ()>,
    {
        let bytes = self.get_prop_raw(code)?;
        let value = codec::decode_exact(&bytes)?;
        Ok(value)
    }

    pub(crate) fn set_prop<T>(&mut self, code: impl Into<u16>, value: &T) -> anyhow::Result<()>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        let bytes = codec::encode(value)?;
        self.set_prop_raw(code, &bytes)?;
        Ok(())
    }

    pub(crate) fn set_prop_for_operation<T>(
        &mut self,
        operation: PtpOperation,
        code: impl Into<u16>,
        value: &T,
    ) -> anyhow::Result<()>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        let prop = code.into();
        let bytes = codec::encode(value)?;
        self.send_for_operation(
            operation,
            CommandCode::SetDevicePropValue,
            &[u32::from(prop)],
            Some(&bytes),
        )?;
        Ok(())
    }

    pub(crate) fn authorize_mutations(
        &mut self,
        operations: &[u16],
        properties: Vec<DevicePropDesc>,
        capability_profile: &'static crate::generated::cameras::CameraFirmwareCapabilityProfile,
    ) {
        self.mutation_authorization = Some(MutationAuthorization::new(
            operations,
            properties,
            capability_profile,
        ));
    }

    pub(crate) fn clear_mutation_authorization(&mut self) {
        self.mutation_authorization = None;
    }

    pub(crate) fn firmware_option_write_value(
        &self,
        option: &str,
        logical_value: &str,
    ) -> anyhow::Result<i32> {
        self.mutation_authorization
            .as_ref()
            .ok_or_else(|| anyhow!("firmware option encoding requires camera preflight"))?
            .capability_profile
            .write_wire_value(option, logical_value)
    }

    pub(crate) fn firmware_capability_profile(
        &self,
    ) -> anyhow::Result<&'static crate::generated::cameras::CameraFirmwareCapabilityProfile> {
        self.mutation_authorization
            .as_ref()
            .map(|authorization| authorization.capability_profile)
            .ok_or_else(|| anyhow!("firmware capability validation requires camera preflight"))
    }

    pub(crate) fn firmware_option_read_logical_value(
        &self,
        option: &str,
        wire_value: i32,
    ) -> anyhow::Result<&'static str> {
        self.mutation_authorization
            .as_ref()
            .ok_or_else(|| anyhow!("firmware option decoding requires camera preflight"))?
            .capability_profile
            .read_logical_value(option, wire_value)
    }

    pub(crate) fn validate_raw_conversion_profile(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let authorization = self.mutation_authorization.as_mut().ok_or_else(|| {
            anyhow!("RAW conversion profile validation requires camera preflight")
        })?;
        authorization.validate_raw_conversion_profile(profile_code, header_padding, fields, bytes)
    }

    pub(crate) fn validate_raw_conversion_read_fingerprint(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        declared_field_count: u16,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let authorization = self.mutation_authorization.as_mut().ok_or_else(|| {
            anyhow!("RAW conversion fingerprint validation requires camera preflight")
        })?;
        authorization.validate_raw_conversion_read_fingerprint(
            profile_code,
            header_padding,
            declared_field_count,
            fields,
            bytes,
        )
    }

    fn validate_mutation(
        &self,
        code: CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        if !is_mutating_command(code) {
            return Ok(());
        }
        validate_mutation_authorization(self.mutation_authorization.as_ref(), code, params, data)
    }
}

fn session_is_safe_to_close(
    poisoned: bool,
    transaction_id: u32,
    camera_processing_active: bool,
) -> bool {
    !poisoned && transaction_id != u32::MAX && !camera_processing_active
}

fn validate_raw_conversion_live_envelope(
    bytes: &[u8],
    expected_profile_code: u32,
    header_padding: usize,
    expected_field_count: u16,
    field_slots: usize,
) -> anyhow::Result<()> {
    let mut reader = Cursor::new(bytes);
    let field_count = i16::read_options(&mut reader, Endian::Little, ())?;
    ensure!(
        field_count >= 0 && u16::try_from(field_count)? == expected_field_count,
        "live RAW conversion field count does not match the firmware descriptor"
    );
    let profile_code = codec::PtpExactString::read_options(&mut reader, Endian::Little, ())?;
    let parsed_profile_code = u32::from_str_radix(profile_code.as_str(), 16)
        .context("decoding live RAW conversion profile code")?;
    ensure!(
        parsed_profile_code == expected_profile_code,
        "live RAW conversion profile code does not match the firmware descriptor"
    );
    let remaining = bytes
        .len()
        .saturating_sub(usize::try_from(reader.position())?);
    let expected_remaining = header_padding
        .checked_add(
            field_slots
                .checked_mul(size_of::<i32>())
                .ok_or_else(|| anyhow!("RAW conversion field geometry overflow"))?,
        )
        .ok_or_else(|| anyhow!("RAW conversion payload geometry overflow"))?;
    ensure!(
        remaining == expected_remaining,
        "live RAW conversion payload geometry does not match the firmware descriptor"
    );
    Ok(())
}

const fn is_mutating_command(code: CommandCode) -> bool {
    matches!(
        code,
        CommandCode::DeleteObject
            | CommandCode::SendObjectInfo
            | CommandCode::SendObject
            | CommandCode::SetDevicePropValue
            | CommandCode::FujiSendObjectInfo
            | CommandCode::FujiSendObject
    )
}

const fn is_read_only_command(code: CommandCode) -> bool {
    matches!(
        code,
        CommandCode::GetDeviceInfo
            | CommandCode::GetObjectHandles
            | CommandCode::GetObjectInfo
            | CommandCode::GetObject
            | CommandCode::GetDevicePropDesc
            | CommandCode::GetDevicePropValue
    )
}

fn validate_mutation_authorization(
    authorization: Option<&MutationAuthorization>,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
) -> anyhow::Result<()> {
    authorization
        .ok_or_else(|| anyhow!("PTP mutation requires a validated camera preflight"))?
        .validate(code, params, data)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint and transaction tuple"
)]
#[cfg(test)]
fn send_with_transport<T: BulkTransport>(
    transport: &T,
    bulk_in: u8,
    bulk_out: u8,
    chunk_size: usize,
    transaction_id: &mut u32,
    poisoned: &mut bool,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
) -> anyhow::Result<Vec<u8>> {
    let mut read_state = BulkReadState::new(chunk_size)?;
    send_with_transport_and_read_state(
        transport,
        bulk_in,
        bulk_out,
        chunk_size,
        &mut read_state,
        transaction_id,
        poisoned,
        code,
        params,
        data,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint, framing, and transaction tuple"
)]
#[cfg(test)]
fn send_with_transport_and_read_state<T: BulkTransport>(
    transport: &T,
    bulk_in: u8,
    bulk_out: u8,
    write_chunk_size: usize,
    read_state: &mut BulkReadState,
    transaction_id: &mut u32,
    poisoned: &mut bool,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
) -> anyhow::Result<Vec<u8>> {
    let deadline = Deadline::new(&SystemClock, PTP_TRANSACTION_TIMEOUT)?;
    send_with_transport_and_deadline(
        transport,
        bulk_in,
        bulk_out,
        write_chunk_size,
        read_state,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        deadline,
        PtpOperation::Standard.timeout_policy(),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint, transaction, and operation tuple"
)]
fn send_with_transport_for_operation<T: BulkTransport>(
    transport: &T,
    bulk_in: u8,
    bulk_out: u8,
    chunk_size: usize,
    read_state: &mut BulkReadState,
    transaction_id: &mut u32,
    poisoned: &mut bool,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
    operation: PtpOperation,
) -> anyhow::Result<Vec<u8>> {
    let policy = operation.timeout_policy();
    let deadline = Deadline::new(&SystemClock, policy.transaction_timeout)?;
    send_with_transport_and_deadline(
        transport,
        bulk_in,
        bulk_out,
        chunk_size,
        read_state,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        deadline,
        policy,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint, transaction, and clock tuple"
)]
#[cfg(test)]
fn send_with_transport_and_clock<T: BulkTransport, C: Clock>(
    transport: &T,
    bulk_in: u8,
    bulk_out: u8,
    chunk_size: usize,
    transaction_id: &mut u32,
    poisoned: &mut bool,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
    clock: &C,
) -> anyhow::Result<Vec<u8>> {
    send_with_transport_for_operation_and_clock(
        transport,
        bulk_in,
        bulk_out,
        chunk_size,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        PtpOperation::Standard,
        clock,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport test seam keeps the operation class and PTP tuple explicit"
)]
#[cfg(test)]
fn send_with_transport_for_operation_and_clock<T: BulkTransport, C: Clock>(
    transport: &T,
    bulk_in: u8,
    bulk_out: u8,
    chunk_size: usize,
    transaction_id: &mut u32,
    poisoned: &mut bool,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
    operation: PtpOperation,
    clock: &C,
) -> anyhow::Result<Vec<u8>> {
    let policy = operation.timeout_policy();
    let deadline = Deadline::new(clock, policy.transaction_timeout)?;
    let mut read_state = BulkReadState::new(chunk_size)?;
    send_with_transport_and_deadline(
        transport,
        bulk_in,
        bulk_out,
        chunk_size,
        &mut read_state,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        deadline,
        policy,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint, framing, transaction, and deadline tuple"
)]
fn send_with_transport_until_and_read_state<T: BulkTransport>(
    transport: &T,
    bulk_in: u8,
    bulk_out: u8,
    write_chunk_size: usize,
    read_state: &mut BulkReadState,
    transaction_id: &mut u32,
    poisoned: &mut bool,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
    expires_at: Instant,
) -> anyhow::Result<Vec<u8>> {
    let deadline = Deadline::until(&SystemClock, expires_at)?;
    send_with_transport_and_deadline(
        transport,
        bulk_in,
        bulk_out,
        write_chunk_size,
        read_state,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        deadline,
        PtpOperation::Polling.timeout_policy(),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint, transaction, deadline, and clock tuple"
)]
#[cfg(test)]
fn send_with_transport_until_and_clock<T: BulkTransport, C: Clock>(
    transport: &T,
    bulk_in: u8,
    bulk_out: u8,
    chunk_size: usize,
    transaction_id: &mut u32,
    poisoned: &mut bool,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
    expires_at: Instant,
    clock: &C,
) -> anyhow::Result<Vec<u8>> {
    let deadline = Deadline::until(clock, expires_at)?;
    let mut read_state = BulkReadState::new(chunk_size)?;
    send_with_transport_and_deadline(
        transport,
        bulk_in,
        bulk_out,
        chunk_size,
        &mut read_state,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        deadline,
        PtpOperation::Polling.timeout_policy(),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call keeps the PTP request and response deadline explicit"
)]
fn send_with_transport_and_deadline<T: BulkTransport, C: Clock>(
    transport: &T,
    bulk_in: u8,
    bulk_out: u8,
    write_chunk_size: usize,
    read_state: &mut BulkReadState,
    transaction_id: &mut u32,
    poisoned: &mut bool,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
    deadline: Deadline<'_, C>,
    policy: PtpTimeoutPolicy,
) -> anyhow::Result<Vec<u8>> {
    ensure!(
        !*poisoned,
        "PTP session is unusable; reopen the camera connection"
    );
    ensure!(
        *transaction_id != u32::MAX,
        "PTP transaction ID space exhausted; reopen the camera connection"
    );
    let current_transaction_id = *transaction_id;

    trace!(
        "PTP tx={current_transaction_id}: code={code:?}, param_count={}, data_len={}",
        params.len(),
        data.map_or(0, <[u8]>::len)
    );

    let payload = encode_command_params(params)?;

    let mut command_dispatched_or_ambiguous = false;
    let command_deadline =
        deadline.phase(PtpDeadlinePhase::CommandWrite, policy.command_idle_timeout)?;
    let command_result = write_container(
        transport,
        bulk_out,
        write_chunk_size,
        ContainerType::Command,
        code,
        &payload,
        current_transaction_id,
        &command_deadline,
        &mut command_dispatched_or_ambiguous,
    );
    if command_dispatched_or_ambiguous {
        *transaction_id = current_transaction_id + 1;
    }
    if let Err(error) = command_result {
        if command_dispatched_or_ambiguous {
            *poisoned = true;
        }
        let context =
            format!("PTP {code:?} tx={current_transaction_id} command write failed: {error}");
        return Err(error).context(context);
    }

    if let Some(data) = data {
        let data_deadline =
            deadline.phase(PtpDeadlinePhase::DataTransfer, policy.data_idle_timeout)?;
        if let Err(error) = write_container(
            transport,
            bulk_out,
            write_chunk_size,
            ContainerType::Data,
            code,
            data,
            current_transaction_id,
            &data_deadline,
            &mut command_dispatched_or_ambiguous,
        ) {
            *poisoned = true;
            let context =
                format!("PTP {code:?} tx={current_transaction_id} data write failed: {error}");
            return Err(error).context(context);
        }
    }

    let result = (|| {
        let mut response = None;
        loop {
            let response_deadline =
                deadline.phase(PtpDeadlinePhase::Response, policy.response_timeout)?;
            let data_deadline =
                deadline.phase(PtpDeadlinePhase::DataTransfer, policy.data_idle_timeout)?;
            let (container, payload) = read_container_from_state_with_deadline(
                transport,
                bulk_in,
                read_state,
                &response_deadline,
                Some(&data_deadline),
            )
            .map_err(|error| {
                let context = format!(
                    "PTP {code:?} tx={current_transaction_id} response read failed: {error}"
                );
                error.context(context)
            })?;
            ensure!(
                container.transaction_id == current_transaction_id,
                "PTP transaction ID mismatch: got {}, expected {current_transaction_id}",
                container.transaction_id
            );
            if container.kind == ContainerType::Response {
                ensure!(
                    !read_state.has_pending_bytes(),
                    "unexpected bytes after PTP response container"
                );
            }

            match (container.kind, container.code) {
                (ContainerType::Data, ContainerCode::Command(container_code)) => {
                    ensure!(
                        container_code == code,
                        "PTP data code mismatch: got {container_code:?}, expected {code:?}"
                    );
                    ensure!(response.is_none(), "received multiple PTP data containers");
                    response = Some(payload);
                }
                (ContainerType::Response, ContainerCode::Response(ResponseCode::Ok)) => {
                    return Ok(response.unwrap_or_default());
                }
                (ContainerType::Response, ContainerCode::Response(response_code)) => {
                    return Err(anyhow!(error::Error::Response(response_code.into())));
                }
                (ContainerType::Response, ContainerCode::Unknown(response_code)) => {
                    return Err(anyhow!(error::Error::Response(response_code)));
                }
                (kind, container_code) => {
                    bail!(
                        "unexpected PTP {kind:?} container with code {container_code:?} for {code:?}"
                    );
                }
            }
        }
    })();

    if let Err(ref cause) = result
        && !matches!(
            cause.downcast_ref::<error::Error>(),
            Some(error::Error::Response(_))
        )
    {
        *poisoned = true;
    }

    result
}

fn encode_command_params(params: &[u32]) -> anyhow::Result<Vec<u8>> {
    let payload_capacity = params
        .len()
        .checked_mul(size_of::<u32>())
        .ok_or_else(|| anyhow!("too many PTP command parameters"))?;
    ensure!(
        payload_capacity <= MAX_PTP_CONTAINER_PAYLOAD_BYTES,
        "PTP command parameter payload {payload_capacity} exceeds maximum {MAX_PTP_CONTAINER_PAYLOAD_BYTES}"
    );
    let mut bytes = Vec::new();
    reserve_bytes(&mut bytes, payload_capacity, "PTP command parameters")?;
    let mut writer = Cursor::new(bytes);
    for param in params {
        param.write_options(&mut writer, Endian::Little, ())?;
    }
    Ok(writer.into_inner())
}

#[expect(
    clippy::too_many_arguments,
    reason = "container writes keep the PTP endpoint, deadline, and clock explicit"
)]
fn write_container<T: BulkTransport, C: Clock>(
    transport: &T,
    bulk_out: u8,
    chunk_size: usize,
    kind: ContainerType,
    code: CommandCode,
    payload: &[u8],
    transaction_id: u32,
    deadline: &Deadline<'_, C>,
    dispatched_or_ambiguous: &mut bool,
) -> anyhow::Result<()> {
    ensure!(
        chunk_size > ContainerInfo::SIZE,
        "PTP chunk size must exceed the container header size"
    );
    let max_chunk_size = ContainerInfo::SIZE
        .checked_add(MAX_PTP_CONTAINER_PAYLOAD_BYTES)
        .ok_or_else(|| anyhow!("PTP maximum chunk size overflow"))?;
    ensure!(
        chunk_size <= max_chunk_size,
        "PTP chunk size {chunk_size} exceeds maximum {max_chunk_size}"
    );
    let container_info = ContainerInfo::new(kind, code, transaction_id, payload.len())?;
    let first_payload_len = min(payload.len(), chunk_size - ContainerInfo::SIZE);
    let first_chunk_len = ContainerInfo::SIZE
        .checked_add(first_payload_len)
        .ok_or_else(|| anyhow!("PTP initial bulk chunk length overflow"))?;
    let mut bytes = Vec::new();
    reserve_bytes(&mut bytes, first_chunk_len, "PTP initial bulk chunk")?;
    let mut writer = Cursor::new(bytes);
    container_info.write_options(&mut writer, Endian::Little, ())?;
    let mut first_chunk = writer.into_inner();
    first_chunk.extend_from_slice(&payload[..first_payload_len]);

    write_all_bulk(
        transport,
        bulk_out,
        &first_chunk,
        deadline,
        dispatched_or_ambiguous,
    )?;
    for chunk in payload[first_payload_len..].chunks(chunk_size) {
        write_all_bulk(
            transport,
            bulk_out,
            chunk,
            deadline,
            dispatched_or_ambiguous,
        )?;
    }
    Ok(())
}

fn reserve_bytes(buffer: &mut Vec<u8>, len: usize, purpose: &str) -> anyhow::Result<()> {
    buffer
        .try_reserve_exact(len)
        .map_err(|error| anyhow!("failed to allocate {purpose} ({len} bytes): {error}"))
}

pub(crate) fn validate_bulk_read_geometry(
    chunk_size: usize,
    max_packet_size: usize,
) -> anyhow::Result<()> {
    ensure!(
        max_packet_size != 0,
        "PTP bulk IN endpoint maximum packet size must be non-zero"
    );
    ensure!(
        chunk_size >= ContainerInfo::SIZE,
        "PTP chunk size must fit the container header"
    );
    ensure!(
        chunk_size <= MAX_PTP_BULK_READ_CHUNK_BYTES,
        "PTP chunk size {chunk_size} exceeds maximum bulk read allocation {MAX_PTP_BULK_READ_CHUNK_BYTES}"
    );
    ensure!(
        chunk_size.is_multiple_of(max_packet_size),
        "PTP chunk size {chunk_size} must be a multiple of the bulk IN endpoint packet size {max_packet_size}"
    );
    Ok(())
}

fn write_all_bulk<T: BulkTransport, C: Clock>(
    transport: &T,
    bulk_out: u8,
    buffer: &[u8],
    deadline: &Deadline<'_, C>,
    dispatched_or_ambiguous: &mut bool,
) -> anyhow::Result<()> {
    let mut written = 0;
    while written < buffer.len() {
        let timeout = deadline.io_timeout()?;
        let n = match transport.write_bulk(bulk_out, &buffer[written..], timeout) {
            Ok(n) => n,
            // rusb reports Timeout as Err only when this call transferred zero bytes, so the
            // same confirmed offset is safe to continue within the current PTP transaction.
            Err(rusb::Error::Timeout) => continue,
            Err(error) => {
                *dispatched_or_ambiguous = true;
                return Err(error.into());
            }
        };
        ensure!(
            n != 0,
            "PTP bulk write completed without transferring bytes"
        );
        *dispatched_or_ambiguous = true;
        written = written
            .checked_add(n)
            .ok_or_else(|| anyhow!("PTP bulk write length overflow"))?;
        ensure!(
            written <= buffer.len(),
            "PTP bulk write exceeded requested length"
        );
        deadline.record_progress()?;
    }
    Ok(())
}

#[cfg(test)]
fn read_container<T: BulkTransport>(
    transport: &T,
    bulk_in: u8,
    chunk_size: usize,
) -> anyhow::Result<(ContainerInfo, Vec<u8>)> {
    let deadline = Deadline::new(&SystemClock, PTP_TRANSACTION_TIMEOUT)?;
    read_container_with_deadline(transport, bulk_in, chunk_size, &deadline, None)
}

#[cfg(test)]
fn read_container_with_deadline<T: BulkTransport, C: Clock>(
    transport: &T,
    bulk_in: u8,
    chunk_size: usize,
    deadline: &Deadline<'_, C>,
    payload_deadline: Option<&Deadline<'_, C>>,
) -> anyhow::Result<(ContainerInfo, Vec<u8>)> {
    let mut read_state = BulkReadState::new(chunk_size)?;
    read_container_from_state_with_deadline(
        transport,
        bulk_in,
        &mut read_state,
        deadline,
        payload_deadline,
    )
}

fn read_container_from_state_with_deadline<T: BulkTransport, C: Clock>(
    transport: &T,
    bulk_in: u8,
    read_state: &mut BulkReadState,
    deadline: &Deadline<'_, C>,
    payload_deadline: Option<&Deadline<'_, C>>,
) -> anyhow::Result<(ContainerInfo, Vec<u8>)> {
    let mut header = [0; ContainerInfo::SIZE];
    read_state.read_exact(
        transport,
        bulk_in,
        &mut header,
        deadline,
        "PTP container header is truncated",
    )?;

    let mut cur = Cursor::new(header);
    let container_info = ContainerInfo::read_options(&mut cur, Endian::Little, ())?;
    let payload_len = container_info.payload_len()?;
    ensure!(
        payload_len <= MAX_PTP_CONTAINER_PAYLOAD_BYTES,
        "PTP container payload length {payload_len} exceeds maximum {MAX_PTP_CONTAINER_PAYLOAD_BYTES}"
    );

    let mut payload = Vec::new();
    reserve_bytes(&mut payload, payload_len, "PTP container payload")?;
    let has_separate_payload_deadline = payload_deadline.is_some();
    let payload_deadline = payload_deadline.unwrap_or(deadline);
    if has_separate_payload_deadline && payload_len != 0 {
        payload_deadline.record_progress()?;
    }
    payload.resize(payload_len, 0);
    read_state.read_exact(
        transport,
        bulk_in,
        &mut payload,
        payload_deadline,
        "PTP payload ended before its declared length",
    )?;

    Ok((container_info, payload))
}

impl Drop for Ptp {
    fn drop(&mut self) {
        if let Err(e) = self.handle.release_interface(self.interface) {
            error!("Failed to release USB interface: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::VecDeque,
        time::{Duration, Instant},
    };

    use super::{
        BulkReadState, BulkTransport, Clock, CommandCode, ContainerCode, ContainerInfo,
        ContainerType, Deadline, DevicePropDataType, DevicePropDesc, DevicePropForm,
        DevicePropValue, MAX_PTP_CONTAINER_PAYLOAD_BYTES, MutationAuthorization, PTP_BULK_TIMEOUT,
        PTP_DATA_IDLE_TIMEOUT, PTP_TRANSACTION_TIMEOUT, PtpDeadlineExceeded, PtpDeadlineKind,
        PtpDeadlinePhase, PtpOperation, ResponseCode, encode_command_params, read_container,
        read_container_with_deadline, send_with_transport, send_with_transport_and_clock,
        send_with_transport_and_read_state, send_with_transport_for_operation_and_clock,
        send_with_transport_until_and_clock, session_is_safe_to_close, validate_bulk_read_geometry,
        validate_mutation_authorization, validate_raw_conversion_live_envelope, write_all_bulk,
        write_container,
    };

    const EMPTY_CAPABILITY_PROFILE: crate::generated::cameras::CameraFirmwareCapabilityProfile =
        crate::generated::cameras::CameraFirmwareCapabilityProfile {
            firmware: "test",
            options: &[],
            raw_conversion: None,
        };
    const RAW_CAPABILITY_PROFILE: crate::generated::cameras::CameraFirmwareCapabilityProfile =
        crate::generated::cameras::CameraFirmwareCapabilityProfile {
            firmware: "test",
            options: &[],
            raw_conversion: Some(crate::generated::cameras::CameraRawConversionDescriptor {
                id: "unverified-test",
                evidence_status:
                    crate::generated::cameras::CameraRawConversionEvidenceStatus::Unverified,
                evidence_manifests: &[],
                usb_modes: &[6],
                camera_state: None,
                read: crate::generated::cameras::CameraRawConversionLayout {
                    profile_code: "1",
                    header_padding: 2,
                    declared_field_count: 1,
                    total_length: 9,
                    fields: &["field"],
                },
                write: Some(crate::generated::cameras::CameraRawConversionLayout {
                    profile_code: "1",
                    header_padding: 2,
                    declared_field_count: 1,
                    total_length: 9,
                    fields: &["field"],
                }),
            }),
        };

    #[test]
    fn healthy_session_with_camera_processing_in_flight_is_not_safe_to_close() {
        assert!(
            !session_is_safe_to_close(false, 1, true),
            "unfinished camera-side processing must suppress automatic CloseSession"
        );
    }

    #[test]
    fn state_changing_ptp_command_requires_preflight_authorization() {
        let error =
            validate_mutation_authorization(None, CommandCode::SendObject, &[0], Some(b"opaque"))
                .expect_err("SendObject must not run without a validated session");

        assert!(error.to_string().contains("validated camera preflight"));
    }

    #[test]
    fn authorized_property_write_still_enforces_dynamic_enumeration() {
        let descriptor = DevicePropDesc {
            property_code: 0xD001,
            data_type: DevicePropDataType::UInt16,
            writable: true,
            factory_default: DevicePropValue::UInt(1),
            current: DevicePropValue::UInt(1),
            form: DevicePropForm::Enumeration(vec![DevicePropValue::UInt(1)]),
        };
        let authorization =
            MutationAuthorization::new(&[0x1016], vec![descriptor], &EMPTY_CAPABILITY_PROFILE);

        let result = validate_mutation_authorization(
            Some(&authorization),
            CommandCode::SetDevicePropValue,
            &[0xD001],
            Some(&2_u16.to_le_bytes()),
        );

        assert!(result.is_err());
    }

    #[test]
    fn raw_image_upload_requires_validated_conversion_profile() {
        let authorization =
            MutationAuthorization::new(&[0x900d], vec![], &EMPTY_CAPABILITY_PROFILE);

        let error = validate_mutation_authorization(
            Some(&authorization),
            CommandCode::FujiSendObject,
            &[],
            Some(b"RAF"),
        )
        .expect_err("RAW image upload must wait until the conversion profile is validated");

        assert!(error.to_string().contains("profile"));
    }

    #[test]
    fn unrelated_property_validation_cannot_authorize_raw_upload() {
        let selector = DevicePropDesc {
            property_code: 0xD18C,
            data_type: DevicePropDataType::UInt16,
            writable: true,
            factory_default: DevicePropValue::UInt(1),
            current: DevicePropValue::UInt(1),
            form: DevicePropForm::None,
        };
        let mut authorization =
            MutationAuthorization::new(&[0x900d], vec![selector], &RAW_CAPABILITY_PROFILE);

        authorization
            .validate_raw_conversion_profile(1, 2, &["field"], &1_u16.to_le_bytes())
            .expect_err("only the RAW conversion profile descriptor can unlock upload");
        let error = validate_mutation_authorization(
            Some(&authorization),
            CommandCode::FujiSendObject,
            &[],
            Some(b"RAF"),
        )
        .expect_err("selector validation must not authorize RAW upload");

        assert!(error.to_string().contains("profile"));
    }

    #[test]
    fn static_raw_signature_cannot_authorize_upload_without_write_evidence() {
        let descriptor = DevicePropDesc {
            property_code: 0xD185,
            data_type: DevicePropDataType::UInt16,
            writable: true,
            factory_default: DevicePropValue::UInt(1),
            current: DevicePropValue::UInt(1),
            form: DevicePropForm::None,
        };
        let mut authorization =
            MutationAuthorization::new(&[0x900d], vec![descriptor], &RAW_CAPABILITY_PROFILE);
        let descriptor_error = authorization
            .validate_raw_conversion_profile(1, 2, &["field"], &1_u16.to_le_bytes())
            .expect_err("an unverified descriptor must not authorize RAW mutation");
        assert!(descriptor_error.to_string().contains("not write-verified"));

        let result = validate_mutation_authorization(
            Some(&authorization),
            CommandCode::FujiSendObject,
            &[],
            Some(b"RAF"),
        );

        assert!(
            result.is_err(),
            "a static RAW signature must not authorize upload without independent write evidence"
        );
    }

    #[test]
    fn live_raw_fingerprint_is_derived_from_observed_payload_bytes() {
        let mut payload = 1_i16.to_le_bytes().to_vec();
        payload.push(1);
        payload.extend_from_slice(&u16::from(b'1').to_le_bytes());
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&42_i32.to_le_bytes());

        validate_raw_conversion_live_envelope(&payload, 1, 2, 1, 1)
            .expect("matching live payload must satisfy the exact envelope");

        payload[..2].copy_from_slice(&2_i16.to_le_bytes());
        let error = validate_raw_conversion_live_envelope(&payload, 1, 2, 1, 1)
            .expect_err("a mismatched live count must fail closed");
        assert!(error.to_string().contains("field count"));
    }

    #[test]
    fn live_raw_fingerprint_rejects_profile_code_and_geometry_drift() {
        let mut payload = 1_i16.to_le_bytes().to_vec();
        payload.push(1);
        payload.extend_from_slice(&u16::from(b'2').to_le_bytes());
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&42_i32.to_le_bytes());

        let code_error = validate_raw_conversion_live_envelope(&payload, 1, 2, 1, 1)
            .expect_err("a mismatched live profile code must fail closed");
        assert!(code_error.to_string().contains("profile code"));

        payload[3..5].copy_from_slice(&u16::from(b'1').to_le_bytes());
        payload.push(0);
        let geometry_error = validate_raw_conversion_live_envelope(&payload, 1, 2, 1, 1)
            .expect_err("trailing live bytes must fail closed");
        assert!(geometry_error.to_string().contains("geometry"));
    }

    #[test]
    fn binrw_encodes_command_parameters_in_wire_order() {
        let encoded = encode_command_params(&[0x01020304, 0xaabbccdd])
            .expect("command parameter encoding must succeed");

        assert_eq!(encoded, [4, 3, 2, 1, 0xdd, 0xcc, 0xbb, 0xaa]);
    }

    struct FakeClock {
        instants: RefCell<VecDeque<Instant>>,
    }

    impl FakeClock {
        fn new(instants: impl IntoIterator<Item = Instant>) -> Self {
            Self {
                instants: RefCell::new(instants.into_iter().collect()),
            }
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.instants
                .borrow_mut()
                .pop_front()
                .expect("test clock exhausted")
        }
    }

    #[derive(Default)]
    struct FakeBulkTransport {
        read_errors: RefCell<VecDeque<Option<rusb::Error>>>,
        reads: RefCell<VecDeque<Vec<u8>>>,
        write_errors: RefCell<VecDeque<Option<rusb::Error>>>,
        write_lengths: RefCell<VecDeque<usize>>,
        writes: RefCell<Vec<Vec<u8>>>,
        timeouts: RefCell<Vec<std::time::Duration>>,
    }

    impl FakeBulkTransport {
        fn with_reads(reads: impl IntoIterator<Item = Vec<u8>>) -> Self {
            Self {
                reads: RefCell::new(reads.into_iter().collect()),
                ..Self::default()
            }
        }
    }

    impl BulkTransport for FakeBulkTransport {
        fn read_bulk(
            &self,
            _endpoint: u8,
            buf: &mut [u8],
            timeout: std::time::Duration,
        ) -> rusb::Result<usize> {
            self.timeouts.borrow_mut().push(timeout);
            if let Some(Some(error)) = self.read_errors.borrow_mut().pop_front() {
                return Err(error);
            }
            let next = self.reads.borrow_mut().pop_front().unwrap_or_default();
            if next.len() > buf.len() {
                return Err(rusb::Error::Overflow);
            }
            buf[..next.len()].copy_from_slice(&next);
            Ok(next.len())
        }

        fn write_bulk(
            &self,
            _endpoint: u8,
            buf: &[u8],
            timeout: std::time::Duration,
        ) -> rusb::Result<usize> {
            self.timeouts.borrow_mut().push(timeout);
            if let Some(Some(error)) = self.write_errors.borrow_mut().pop_front() {
                return Err(error);
            }
            self.writes.borrow_mut().push(buf.to_vec());
            Ok(self
                .write_lengths
                .borrow_mut()
                .pop_front()
                .unwrap_or(buf.len()))
        }
    }

    fn container(
        kind: ContainerType,
        code: ContainerCode,
        transaction_id: u32,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut bytes = crate::ptp::codec::encode(&ContainerInfo {
            total_len: (ContainerInfo::SIZE + payload.len()).try_into().unwrap(),
            kind,
            code,
            transaction_id,
        })
        .unwrap();
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn transport_failure_reports_safe_ptp_operation_context() {
        let transport = FakeBulkTransport::default();
        transport
            .write_errors
            .borrow_mut()
            .push_back(Some(rusb::Error::NoDevice));
        let mut transaction_id = 17;
        let mut poisoned = false;

        let error = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            Some(b"camera-private-payload"),
        )
        .unwrap_err();
        let message = error.to_string();

        assert!(
            message.contains("GetDeviceInfo")
                && message.contains("tx=17")
                && message.contains("command write")
                && !message.contains("camera-private-payload"),
            "PTP transport error lacked safe operation context or exposed payload: {message:?}"
        );
        assert!(poisoned, "USB disconnect must poison the PTP session");
    }

    #[test]
    fn later_transport_failures_report_safe_ptp_operation_context() {
        enum FailurePhase {
            DataWrite,
            ResponseRead,
        }

        let mut failures = Vec::new();
        for (failure_phase, expected_context) in [
            (FailurePhase::DataWrite, "data write"),
            (FailurePhase::ResponseRead, "response read"),
        ] {
            let transport = FakeBulkTransport::default();
            match failure_phase {
                FailurePhase::DataWrite => transport
                    .write_errors
                    .borrow_mut()
                    .extend([None, Some(rusb::Error::NoDevice)]),
                FailurePhase::ResponseRead => transport
                    .read_errors
                    .borrow_mut()
                    .push_back(Some(rusb::Error::NoDevice)),
            }
            let mut transaction_id = 29;
            let mut poisoned = false;

            let error = send_with_transport(
                &transport,
                0x81,
                0x02,
                1024,
                &mut transaction_id,
                &mut poisoned,
                CommandCode::SetDevicePropValue,
                &[0xD001],
                Some(b"camera-private-payload"),
            )
            .unwrap_err();
            let message = error.to_string();
            if !message.contains("SetDevicePropValue")
                || !message.contains("tx=29")
                || !message.contains(expected_context)
                || message.contains("camera-private-payload")
            {
                failures.push(format!("{expected_context}: {message:?}"));
            }
            if !poisoned {
                failures.push(format!("{expected_context}: session was not poisoned"));
            }
        }

        assert!(
            failures.is_empty(),
            "PTP transport errors lacked safe operation context or exposed payload:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn response_timeout_before_deadline_keeps_waiting_without_poisoning_session() {
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            0,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([response]);
        transport
            .read_errors
            .borrow_mut()
            .push_back(Some(rusb::Error::Timeout));
        let mut transaction_id = 0;
        let mut poisoned = false;

        let result = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        );

        assert!(
            result.is_ok(),
            "a zero-byte USB timeout before the response deadline must keep waiting: {result:?}"
        );
        assert!(
            !poisoned,
            "a recovered response wait must keep the session healthy"
        );
    }

    #[test]
    fn rejects_payload_larger_than_protocol_limit_before_allocation() {
        let header = crate::ptp::codec::encode(&ContainerInfo {
            total_len: (ContainerInfo::SIZE + MAX_PTP_CONTAINER_PAYLOAD_BYTES + 1)
                .try_into()
                .unwrap(),
            kind: ContainerType::Data,
            code: ContainerCode::Command(CommandCode::GetObject),
            transaction_id: 1,
        })
        .unwrap();
        let transport = FakeBulkTransport::with_reads([header]);

        let err = read_container(&transport, 0x81, 1024).unwrap_err();

        assert!(err.to_string().contains("exceeds maximum"));
    }

    #[test]
    fn rejects_short_payload_instead_of_returning_truncated_data() {
        let header = container(
            ContainerType::Data,
            ContainerCode::Command(CommandCode::GetObject),
            1,
            &[0xAA],
        );
        let transport =
            FakeBulkTransport::with_reads([header[..ContainerInfo::SIZE].to_vec(), vec![]]);

        let err = read_container(&transport, 0x81, 1024).unwrap_err();

        assert!(err.to_string().contains("ended before its declared length"));
    }

    #[test]
    fn accumulates_partial_bulk_reads_into_a_full_header_under_one_deadline() {
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            7,
            &[],
        );

        for split_at in 1..ContainerInfo::SIZE {
            let transport = FakeBulkTransport::with_reads([
                response[..split_at].to_vec(),
                response[split_at..].to_vec(),
            ]);
            let start = Instant::now();
            let clock = FakeClock::new([start; 3]);
            let deadline = Deadline::new(&clock, PTP_TRANSACTION_TIMEOUT)
                .expect("test deadline must be representable");

            let (header, payload) =
                read_container_with_deadline(&transport, 0x81, 1024, &deadline, None)
                    .expect("fragmented header must be assembled");

            assert_eq!(header.transaction_id, 7, "split at {split_at}");
            assert!(payload.is_empty(), "split at {split_at}");
            assert_eq!(
                *transport.timeouts.borrow(),
                vec![PTP_BULK_TIMEOUT; 2],
                "split at {split_at}",
            );
        }
    }

    #[test]
    fn reads_header_and_payload_from_one_bulk_transfer() {
        let payload = [0x11, 0x22, 0x33, 0x44];
        let packet = container(
            ContainerType::Data,
            ContainerCode::Command(CommandCode::GetObject),
            7,
            &payload,
        );
        let transport = FakeBulkTransport::with_reads([packet]);

        let (header, received_payload) = read_container(&transport, 0x81, 1024)
            .expect("combined PTP header and payload must be accepted in one bulk transfer");

        assert_eq!(header.transaction_id, 7);
        assert_eq!(received_payload, payload);
    }

    #[test]
    fn reads_data_and_response_containers_from_one_bulk_transfer() {
        let payload = [0x11, 0x22, 0x33, 0x44];
        let mut bulk_read = container(
            ContainerType::Data,
            ContainerCode::Command(CommandCode::GetObject),
            7,
            &payload,
        );
        bulk_read.extend_from_slice(&container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            7,
            &[],
        ));
        let transport = FakeBulkTransport::with_reads([bulk_read]);
        let mut transaction_id = 7;
        let mut poisoned = false;

        let received_payload = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetObject,
            &[],
            None,
        )
        .expect("coalesced PTP data and response containers must both be parsed");

        assert_eq!(received_payload, payload);
        assert!(!poisoned);
    }

    #[test]
    fn skips_one_terminating_zlp_after_a_full_bulk_read() {
        const CHUNK_SIZE: usize = 1024;

        let payload = vec![0x5a; CHUNK_SIZE - ContainerInfo::SIZE];
        let data = container(
            ContainerType::Data,
            ContainerCode::Command(CommandCode::GetObject),
            7,
            &payload,
        );
        assert_eq!(data.len(), CHUNK_SIZE);
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            7,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([data, vec![], response]);
        let mut transaction_id = 7;
        let mut poisoned = false;

        let received_payload = send_with_transport(
            &transport,
            0x81,
            0x02,
            CHUNK_SIZE,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetObject,
            &[],
            None,
        )
        .expect("one terminating ZLP after a full bulk read must be skipped");

        assert_eq!(received_payload, payload);
        assert!(!poisoned);
    }

    #[test]
    fn rejects_repeated_zero_progress_after_a_full_bulk_read() {
        const CHUNK_SIZE: usize = 1024;

        let data = container(
            ContainerType::Data,
            ContainerCode::Command(CommandCode::GetObject),
            7,
            &vec![0x5a; CHUNK_SIZE - ContainerInfo::SIZE],
        );
        let transport = FakeBulkTransport::with_reads([data, vec![], vec![]]);
        let mut transaction_id = 7;
        let mut poisoned = false;

        let error = send_with_transport(
            &transport,
            0x81,
            0x02,
            CHUNK_SIZE,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetObject,
            &[],
            None,
        )
        .expect_err("a second zero-progress read must fail the transaction");

        assert!(error.to_string().contains("header is truncated"));
        assert!(poisoned);
    }

    #[test]
    fn carries_a_terminating_zlp_across_read_buffer_resize() {
        const CHUNK_SIZE: usize = 1024;

        let first_payload = vec![0x5a; CHUNK_SIZE - 2 * ContainerInfo::SIZE];
        let mut first_read = container(
            ContainerType::Data,
            ContainerCode::Command(CommandCode::GetObject),
            7,
            &first_payload,
        );
        first_read.extend_from_slice(&container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            7,
            &[],
        ));
        assert_eq!(first_read.len(), CHUNK_SIZE);
        let second_response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            8,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([first_read, vec![], second_response]);
        let mut transaction_id = 7;
        let mut poisoned = false;
        let mut read_state = BulkReadState::new(CHUNK_SIZE).expect("valid read geometry");

        let received_payload = send_with_transport_and_read_state(
            &transport,
            0x81,
            0x02,
            CHUNK_SIZE,
            &mut read_state,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetObject,
            &[],
            None,
        )
        .expect("first transaction must consume the full read window");
        assert_eq!(received_payload, first_payload);

        read_state = read_state
            .resized(CHUNK_SIZE * 2)
            .expect("promotion must preserve the bulk boundary state");

        send_with_transport_and_read_state(
            &transport,
            0x81,
            0x02,
            CHUNK_SIZE,
            &mut read_state,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .expect("the next transaction must skip the preceding terminating ZLP");

        assert!(!poisoned);
    }

    #[test]
    fn rejects_trailing_bytes_after_the_response_container() {
        let mut bulk_read = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            7,
            &[],
        );
        bulk_read.push(0xff);
        let transport = FakeBulkTransport::with_reads([bulk_read]);
        let mut transaction_id = 7;
        let mut poisoned = false;

        let error = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .expect_err("trailing bytes after a response must poison the session");

        assert!(error.to_string().contains("bytes after PTP response"));
        assert!(poisoned);
    }

    #[test]
    fn rejects_bulk_read_buffer_that_is_not_packet_aligned() {
        let error = validate_bulk_read_geometry(1000, 512)
            .expect_err("bulk read buffers must align to endpoint packets");

        assert!(error.to_string().contains("multiple"));
    }

    #[test]
    fn rejects_bulk_read_window_above_transport_allocation_budget() {
        const MAX_BULK_READ_WINDOW_BYTES: usize = 16 * 1024 * 1024;

        let error = validate_bulk_read_geometry(MAX_BULK_READ_WINDOW_BYTES + 512, 512)
            .expect_err("bulk read windows must stay within the transport allocation budget");

        assert!(error.to_string().contains("exceeds maximum"));
    }

    #[test]
    fn rejects_response_for_a_different_transaction_and_poisons_the_session() {
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            42,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([response]);
        let mut transaction_id = 41;
        let mut poisoned = false;

        let err = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .unwrap_err();

        assert!(err.to_string().contains("transaction ID mismatch"));
        assert_eq!(transaction_id, 42);
        assert!(poisoned);
    }

    #[test]
    fn refuses_to_reuse_session_after_ambiguous_protocol_error() {
        let wrong_transaction = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            1,
            &[],
        );
        let otherwise_valid_next_response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            1,
            &[],
        );
        let transport =
            FakeBulkTransport::with_reads([wrong_transaction, otherwise_valid_next_response]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let first_error = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .unwrap_err();
        assert!(first_error.to_string().contains("transaction ID mismatch"));

        let second_error = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .unwrap_err();

        assert!(second_error.to_string().contains("session is unusable"));
        assert_eq!(transport.writes.borrow().len(), 1);
    }

    #[test]
    fn keeps_session_reusable_after_well_framed_device_busy_response() {
        let error_response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::DeviceBusy),
            0,
            &[],
        );
        let success_response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            1,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([error_response, success_response]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .unwrap_err();

        send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .unwrap();

        assert_eq!(transaction_id, 2);
        assert!(!poisoned);
        assert_eq!(transport.writes.borrow().len(), 2);
    }

    #[test]
    fn keeps_session_reusable_after_unknown_vendor_response() {
        let mut vendor_error_response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::GeneralError),
            0,
            &[],
        );
        vendor_error_response[6..8].copy_from_slice(&0xA001_u16.to_le_bytes());
        let success_response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            1,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([vendor_error_response, success_response]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let error = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .expect_err("vendor response must be reported");

        assert!(matches!(
            error.downcast_ref::<super::error::Error>(),
            Some(super::error::Error::Response(0xA001))
        ));
        assert!(!poisoned);

        send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .expect("known-good response after vendor error must be accepted");

        assert_eq!(transaction_id, 2);
        assert!(!poisoned);
    }

    #[test]
    fn enforces_one_absolute_deadline_across_fragmented_io() {
        let data = container(
            ContainerType::Data,
            ContainerCode::Command(CommandCode::GetDeviceInfo),
            0,
            &[0xAA],
        );
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            0,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([
            data[..ContainerInfo::SIZE].to_vec(),
            data[ContainerInfo::SIZE..].to_vec(),
            response,
        ]);
        let start = Instant::now();
        let clock = FakeClock::new([
            start,
            start,
            start,
            start,
            start,
            start,
            start,
            start,
            start,
            start + PTP_TRANSACTION_TIMEOUT + Duration::from_millis(1),
        ]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let error = send_with_transport_and_clock(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
            &clock,
        )
        .unwrap_err();

        assert!(
            error
                .chain()
                .any(|cause| cause.to_string().contains("transaction deadline exceeded"))
        );
        assert!(poisoned, "an in-flight timeout must poison the PTP session");
    }

    #[test]
    fn supplied_earlier_deadline_overrides_default_transaction_budget() {
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            0,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([response]);
        let start = Instant::now();
        let clock = FakeClock::new([
            start,
            start,
            start,
            start,
            start,
            start + Duration::from_secs(2),
        ]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let error = send_with_transport_until_and_clock(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
            start + Duration::from_secs(1),
            &clock,
        )
        .unwrap_err();

        assert!(error.to_string().contains("transaction deadline exceeded"));
        assert_eq!(transport.writes.borrow().len(), 1);
        assert!(poisoned);
    }

    #[test]
    fn supplied_operation_deadline_later_than_default_remains_effective() {
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            0,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([response]);
        let start = Instant::now();
        let delayed_response = start + PTP_TRANSACTION_TIMEOUT + Duration::from_secs(1);
        let clock = FakeClock::new([
            start,
            start,
            start,
            start,
            start,
            delayed_response,
            delayed_response,
        ]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let result = send_with_transport_until_and_clock(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
            start + PTP_TRANSACTION_TIMEOUT + Duration::from_secs(60),
            &clock,
        );

        assert!(
            result.is_ok(),
            "the supplied operation deadline must remain effective beyond the legacy default: {result:?}"
        );
        assert!(
            !poisoned,
            "a response before the operation deadline must keep the session healthy"
        );
    }

    #[test]
    fn large_transfer_accepts_response_after_legacy_transaction_deadline() {
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            0,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([response]);
        let start = Instant::now();
        let delayed_response = start + PTP_TRANSACTION_TIMEOUT + Duration::from_secs(1);
        let clock = FakeClock::new([
            start,
            start,
            start,
            start,
            start,
            start,
            delayed_response,
            delayed_response,
        ]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let result = send_with_transport_for_operation_and_clock(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
            PtpOperation::LargeTransfer,
            &clock,
        );

        assert!(
            result.is_ok(),
            "LargeTransfer must keep waiting beyond the legacy transaction deadline: {result:?}"
        );
    }

    #[test]
    fn expiry_before_first_bulk_write_keeps_session_healthy() {
        let transport = FakeBulkTransport::default();
        let start = Instant::now();
        let clock = FakeClock::new([start, start + Duration::from_secs(2)]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let error = send_with_transport_until_and_clock(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
            start + Duration::from_secs(1),
            &clock,
        )
        .unwrap_err();

        assert!(
            error
                .chain()
                .any(|cause| cause.to_string().contains("transaction deadline exceeded"))
        );
        assert!(transport.writes.borrow().is_empty());
        assert_eq!(transaction_id, 0);
        assert!(!poisoned);
    }

    #[test]
    fn sub_millisecond_remaining_before_first_bulk_write_keeps_session_healthy() {
        let transport = FakeBulkTransport::default();
        let start = Instant::now();
        let clock = FakeClock::new([
            start,
            start + Duration::from_micros(500),
            start + Duration::from_millis(1),
        ]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let error = send_with_transport_until_and_clock(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
            start + Duration::from_millis(1),
            &clock,
        )
        .unwrap_err();

        assert!(
            error
                .chain()
                .any(|cause| cause.to_string().contains("transaction deadline exceeded"))
        );
        assert!(transport.writes.borrow().is_empty());
        assert_eq!(transaction_id, 0);
        assert!(!poisoned);
    }

    #[test]
    fn rejects_data_container_for_a_different_operation_and_poisons_the_session() {
        let data = container(
            ContainerType::Data,
            ContainerCode::Command(CommandCode::GetObject),
            0,
            &[0xAA],
        );
        let transport = FakeBulkTransport::with_reads([
            data[..ContainerInfo::SIZE].to_vec(),
            data[ContainerInfo::SIZE..].to_vec(),
        ]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let err = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .unwrap_err();

        assert!(err.to_string().contains("data code mismatch"));
        assert_eq!(transaction_id, 1);
        assert!(poisoned);
    }

    #[test]
    fn completes_partial_bulk_writes_with_a_finite_timeout() {
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            0,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([response]);
        transport.write_lengths.borrow_mut().extend([3, 9]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let response = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        )
        .unwrap();

        assert!(response.is_empty());
        assert_eq!(transaction_id, 1);
        let writes = transport.writes.borrow();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].len(), ContainerInfo::SIZE);
        assert_eq!(writes[1].len(), ContainerInfo::SIZE - 3);
        assert!(
            transport
                .timeouts
                .borrow()
                .iter()
                .all(|timeout| *timeout == PTP_BULK_TIMEOUT)
        );
    }

    #[test]
    fn zero_byte_write_timeout_before_idle_deadline_retries_same_offset() {
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            0,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([response]);
        transport
            .write_errors
            .borrow_mut()
            .extend([Some(rusb::Error::Timeout), None]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let result = send_with_transport(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetDeviceInfo,
            &[],
            None,
        );

        assert!(
            result.is_ok(),
            "a zero-byte write timeout before the idle deadline must retry the same offset: {result:?}"
        );
        assert_eq!(transaction_id, 1);
        assert_eq!(
            transport.writes.borrow().len(),
            1,
            "the PTP command must not be replayed after a zero-byte timeout"
        );
        assert!(!poisoned);
    }

    #[test]
    fn zero_byte_write_timeouts_until_idle_deadline_remain_pre_dispatch() {
        let transport = FakeBulkTransport::default();
        transport
            .write_errors
            .borrow_mut()
            .push_back(Some(rusb::Error::Timeout));
        let start = Instant::now();
        let clock = FakeClock::new([start, start, start + Duration::from_secs(11)]);
        let deadline = Deadline::with_idle_timeout(
            &clock,
            start + Duration::from_secs(30),
            Duration::from_secs(10),
        )
        .expect("idle deadline must be representable");
        let mut dispatched_or_ambiguous = false;

        let error = write_all_bulk(
            &transport,
            0x02,
            &[0xAA],
            &deadline,
            &mut dispatched_or_ambiguous,
        )
        .expect_err("a run of zero-byte timeouts must stop at the idle deadline");

        assert!(error.to_string().contains("deadline exceeded"));
        assert!(
            !dispatched_or_ambiguous,
            "rusb Timeout confirms that this write call transferred zero bytes"
        );
    }

    #[test]
    fn idle_deadline_error_identifies_transfer_phase_and_kind() {
        let start = Instant::now();
        let clock = FakeClock::new([start, start + Duration::from_secs(11)]);
        let deadline = Deadline::with_idle_timeout(
            &clock,
            start + Duration::from_secs(30),
            Duration::from_secs(10),
        )
        .expect("idle deadline must be representable");

        let error = deadline
            .io_timeout()
            .expect_err("expired idle deadline must be classified");
        let deadline_error = error
            .downcast_ref::<PtpDeadlineExceeded>()
            .expect("deadline expiry must retain a typed cause");

        assert_eq!(deadline_error.phase, PtpDeadlinePhase::DataTransfer);
        assert_eq!(deadline_error.kind, PtpDeadlineKind::Idle);
    }

    #[test]
    fn partial_write_progress_refreshes_idle_deadline_before_next_write() {
        let transport = FakeBulkTransport::default();
        transport.write_lengths.borrow_mut().extend([3, 3]);
        let start = Instant::now();
        let clock = FakeClock::new([
            start,
            start + Duration::from_secs(9),
            start + Duration::from_secs(11),
            start + Duration::from_secs(12),
            start + Duration::from_secs(12),
        ]);
        let deadline = Deadline::with_idle_timeout(
            &clock,
            start + Duration::from_secs(30),
            Duration::from_secs(10),
        )
        .expect("idle deadline must be representable");
        let mut write_attempted = false;

        let result = write_all_bulk(
            &transport,
            0x02,
            &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
            &deadline,
            &mut write_attempted,
        );

        assert!(
            result.is_ok(),
            "confirmed partial-write progress must refresh the idle deadline: {result:?}"
        );
    }

    #[test]
    fn large_inbound_payload_stops_after_data_idle_deadline_without_progress() {
        let data = container(
            ContainerType::Data,
            ContainerCode::Command(CommandCode::GetObject),
            0,
            &[0xAA],
        );
        let response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::Ok),
            0,
            &[],
        );
        let transport = FakeBulkTransport::with_reads([
            data[..ContainerInfo::SIZE].to_vec(),
            data[ContainerInfo::SIZE..].to_vec(),
            response,
        ]);
        transport
            .read_errors
            .borrow_mut()
            .extend([None, Some(rusb::Error::Timeout)]);
        let start = Instant::now();
        let stalled = start + PTP_DATA_IDLE_TIMEOUT + Duration::from_secs(1);
        let clock = FakeClock::new([
            start,
            start,
            start,
            start,
            start,
            start,
            start,
            start,
            start,
            start + Duration::from_secs(1),
            stalled,
        ]);
        let mut transaction_id = 0;
        let mut poisoned = false;

        let error = send_with_transport_for_operation_and_clock(
            &transport,
            0x81,
            0x02,
            1024,
            &mut transaction_id,
            &mut poisoned,
            CommandCode::GetObject,
            &[1],
            None,
            PtpOperation::LargeTransfer,
            &clock,
        )
        .expect_err("an inbound payload with no byte progress must hit the data idle deadline");

        assert!(error.to_string().contains("deadline exceeded"));
        assert!(
            poisoned,
            "an interrupted inbound container must poison the session"
        );
    }

    #[test]
    fn rejects_chunk_size_above_allocation_budget_before_bulk_write() {
        let transport = FakeBulkTransport::default();
        let start = Instant::now();
        let clock = FakeClock::new([start, start]);
        let deadline = Deadline::new(&clock, PTP_TRANSACTION_TIMEOUT)
            .expect("test deadline must be representable");
        let mut write_attempted = false;

        let error = write_container(
            &transport,
            0x02,
            MAX_PTP_CONTAINER_PAYLOAD_BYTES + ContainerInfo::SIZE + 1,
            ContainerType::Command,
            CommandCode::GetDeviceInfo,
            &[],
            0,
            &deadline,
            &mut write_attempted,
        )
        .expect_err("oversized chunk must be rejected before allocation or I/O");

        assert!(error.to_string().contains("exceeds maximum"));
        assert!(transport.writes.borrow().is_empty());
        assert!(!write_attempted);
    }
}
