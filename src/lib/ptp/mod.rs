pub mod codec;
pub mod container;
pub mod error;
pub mod option;
pub mod props;
pub mod structs;

pub use container::*;
pub use props::*;
pub use structs::*;

use std::{
    cmp::min,
    io::Cursor,
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow, bail, ensure};
use binrw::{BinRead, BinWrite, Endian};
use log::{debug, error, trace};
use rusb::GlobalContext;

const PTP_BULK_TIMEOUT: Duration = Duration::from_secs(10);
const MIN_PTP_BULK_TIMEOUT: Duration = Duration::from_millis(1);
const PTP_TRANSACTION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const MAX_PTP_CONTAINER_PAYLOAD_BYTES: usize = 128 * 1024 * 1024;

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
    expires_at: Instant,
}

impl<'a, C: Clock> Deadline<'a, C> {
    fn new(clock: &'a C, timeout: Duration) -> anyhow::Result<Self> {
        let expires_at = clock
            .now()
            .checked_add(timeout)
            .ok_or_else(|| anyhow!("PTP transaction deadline overflow"))?;
        Ok(Self { clock, expires_at })
    }

    fn until(clock: &'a C, expires_at: Instant) -> anyhow::Result<Self> {
        let transaction_expires_at = clock
            .now()
            .checked_add(PTP_TRANSACTION_TIMEOUT)
            .ok_or_else(|| anyhow!("PTP transaction deadline overflow"))?;
        Ok(Self {
            clock,
            expires_at: min(expires_at, transaction_expires_at),
        })
    }

    fn io_timeout(&self) -> anyhow::Result<Duration> {
        let remaining = self.expires_at.saturating_duration_since(self.clock.now());
        ensure!(
            remaining >= MIN_PTP_BULK_TIMEOUT,
            "PTP transaction deadline exceeded"
        );
        Ok(min(PTP_BULK_TIMEOUT, remaining))
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
    pub bus: u8,
    pub address: u8,
    pub interface: u8,
    pub bulk_in: u8,
    pub bulk_out: u8,
    pub handle: rusb::DeviceHandle<GlobalContext>,
    pub transaction_id: u32,
    pub chunk_size: usize,
    pub(crate) poisoned: bool,
}

impl Ptp {
    pub(crate) fn is_healthy(&self) -> bool {
        !self.poisoned && self.transaction_id != u32::MAX
    }

    pub fn send(
        &mut self,
        code: CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        let response = send_with_transport(
            &self.handle,
            self.bulk_in,
            self.bulk_out,
            self.chunk_size,
            &mut self.transaction_id,
            &mut self.poisoned,
            code,
            params,
            data,
        )?;
        trace!("PTP tx complete with response length {}", response.len());

        Ok(response)
    }

    pub(crate) fn send_until(
        &mut self,
        deadline: Instant,
        code: CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<Vec<u8>> {
        let response = send_with_transport_until(
            &self.handle,
            self.bulk_in,
            self.bulk_out,
            self.chunk_size,
            &mut self.transaction_id,
            &mut self.poisoned,
            code,
            params,
            data,
            deadline,
        )?;
        trace!("PTP tx complete with response length {}", response.len());

        Ok(response)
    }

    pub fn open_session(&mut self, session_id: u32) -> anyhow::Result<()> {
        debug!("Opening PTP session");
        self.send(CommandCode::OpenSession, &[session_id], None)?;
        Ok(())
    }

    pub fn close_session(&mut self, _: u32) -> anyhow::Result<()> {
        debug!("Closing PTP session");
        self.send(CommandCode::CloseSession, &[], None)?;
        Ok(())
    }

    pub(crate) fn close_session_until(&mut self, deadline: Instant) -> anyhow::Result<()> {
        debug!("Closing PTP session");
        self.send_until(deadline, CommandCode::CloseSession, &[], None)?;
        Ok(())
    }

    pub fn get_info(&mut self) -> anyhow::Result<DeviceInfo> {
        debug!("Retrieving device info");
        let response = self.send(CommandCode::GetDeviceInfo, &[], None)?;
        let info = codec::decode_exact(&response)?;
        Ok(info)
    }

    pub fn get_prop_raw(&mut self, prop: impl Into<u16>) -> anyhow::Result<Vec<u8>> {
        let prop = prop.into();
        debug!("Getting device prop: 0x{prop:04x}");
        let response = self.send(CommandCode::GetDevicePropValue, &[u32::from(prop)], None)?;
        Ok(response)
    }

    pub fn set_prop_raw(&mut self, prop: impl Into<u16>, value: &[u8]) -> anyhow::Result<Vec<u8>> {
        let prop = prop.into();
        debug!("Setting device prop: 0x{prop:04x}");
        let response = self.send(
            CommandCode::SetDevicePropValue,
            &[u32::from(prop)],
            Some(value),
        )?;
        Ok(response)
    }

    pub fn get_prop<T>(&mut self, code: impl Into<u16>) -> anyhow::Result<T>
    where
        T: for<'a> BinRead<Args<'a> = ()>,
    {
        let bytes = self.get_prop_raw(code)?;
        let value = codec::decode_exact(&bytes)?;
        Ok(value)
    }

    pub fn set_prop<T>(&mut self, code: impl Into<u16>, value: &T) -> anyhow::Result<()>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        let bytes = codec::encode(value)?;
        self.set_prop_raw(code, &bytes)?;
        Ok(())
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint and transaction tuple"
)]
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
    send_with_transport_and_clock(
        transport,
        bulk_in,
        bulk_out,
        chunk_size,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        &SystemClock,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint, transaction, and clock tuple"
)]
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
    let deadline = Deadline::new(clock, PTP_TRANSACTION_TIMEOUT)?;
    send_with_transport_and_deadline(
        transport,
        bulk_in,
        bulk_out,
        chunk_size,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        deadline,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint, transaction, and deadline tuple"
)]
fn send_with_transport_until<T: BulkTransport>(
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
) -> anyhow::Result<Vec<u8>> {
    send_with_transport_until_and_clock(
        transport,
        bulk_in,
        bulk_out,
        chunk_size,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        expires_at,
        &SystemClock,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the transport call mirrors the PTP endpoint, transaction, deadline, and clock tuple"
)]
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
    send_with_transport_and_deadline(
        transport,
        bulk_in,
        bulk_out,
        chunk_size,
        transaction_id,
        poisoned,
        code,
        params,
        data,
        deadline,
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
    chunk_size: usize,
    transaction_id: &mut u32,
    poisoned: &mut bool,
    code: CommandCode,
    params: &[u32],
    data: Option<&[u8]>,
    deadline: Deadline<'_, C>,
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
        "PTP tx={current_transaction_id}: code={code:?}, params={params:?}, data_len={}",
        data.map_or(0, <[u8]>::len)
    );

    let payload = encode_command_params(params)?;

    let mut command_write_attempted = false;
    let command_result = write_container(
        transport,
        bulk_out,
        chunk_size,
        ContainerType::Command,
        code,
        &payload,
        current_transaction_id,
        &deadline,
        &mut command_write_attempted,
    );
    if command_write_attempted {
        *transaction_id = current_transaction_id + 1;
    }
    if let Err(error) = command_result {
        if command_write_attempted {
            *poisoned = true;
        }
        let context =
            format!("PTP {code:?} tx={current_transaction_id} command write failed: {error}");
        return Err(error).context(context);
    }

    if let Some(data) = data
        && let Err(error) = write_container(
            transport,
            bulk_out,
            chunk_size,
            ContainerType::Data,
            code,
            data,
            current_transaction_id,
            &deadline,
            &mut command_write_attempted,
        )
    {
        *poisoned = true;
        let context =
            format!("PTP {code:?} tx={current_transaction_id} data write failed: {error}");
        return Err(error).context(context);
    }

    let result = (|| {
        let mut response = None;
        loop {
            let (container, payload) = read_container_with_deadline(
                transport, bulk_in, chunk_size, &deadline,
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
    write_attempted: &mut bool,
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

    write_all_bulk(transport, bulk_out, &first_chunk, deadline, write_attempted)?;
    for chunk in payload[first_payload_len..].chunks(chunk_size) {
        write_all_bulk(transport, bulk_out, chunk, deadline, write_attempted)?;
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
    write_attempted: &mut bool,
) -> anyhow::Result<()> {
    let mut written = 0;
    while written < buffer.len() {
        let timeout = deadline.io_timeout()?;
        *write_attempted = true;
        let n = transport.write_bulk(bulk_out, &buffer[written..], timeout)?;
        ensure!(
            n != 0,
            "PTP bulk write completed without transferring bytes"
        );
        written = written
            .checked_add(n)
            .ok_or_else(|| anyhow!("PTP bulk write length overflow"))?;
        ensure!(
            written <= buffer.len(),
            "PTP bulk write exceeded requested length"
        );
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
    read_container_with_deadline(transport, bulk_in, chunk_size, &deadline)
}

fn read_container_with_deadline<T: BulkTransport, C: Clock>(
    transport: &T,
    bulk_in: u8,
    chunk_size: usize,
    deadline: &Deadline<'_, C>,
) -> anyhow::Result<(ContainerInfo, Vec<u8>)> {
    ensure!(
        chunk_size >= ContainerInfo::SIZE,
        "PTP chunk size must fit the container header"
    );
    let mut chunk = Vec::new();
    reserve_bytes(&mut chunk, chunk_size, "PTP bulk read chunk")?;
    chunk.resize(chunk_size, 0);

    let mut initial = Vec::new();
    reserve_bytes(&mut initial, chunk_size, "PTP initial bulk read")?;
    while initial.len() < ContainerInfo::SIZE {
        let n = transport.read_bulk(bulk_in, &mut chunk, deadline.io_timeout()?)?;
        ensure!(n != 0, "PTP container header is truncated");
        initial.extend_from_slice(&chunk[..n]);
    }

    let mut cur = Cursor::new(&initial[..ContainerInfo::SIZE]);
    let container_info = ContainerInfo::read_options(&mut cur, Endian::Little, ())?;
    let payload_len = container_info.payload_len()?;
    ensure!(
        payload_len <= MAX_PTP_CONTAINER_PAYLOAD_BYTES,
        "PTP container payload length {payload_len} exceeds maximum {MAX_PTP_CONTAINER_PAYLOAD_BYTES}"
    );

    let mut payload = Vec::new();
    reserve_bytes(&mut payload, payload_len, "PTP container payload")?;
    let initial_payload = &initial[ContainerInfo::SIZE..];
    ensure!(
        initial_payload.len() <= payload_len,
        "PTP payload exceeded its declared length"
    );
    payload.extend_from_slice(initial_payload);

    while payload.len() < payload_len {
        let remaining = payload_len - payload.len();
        let n = transport.read_bulk(bulk_in, &mut chunk, deadline.io_timeout()?)?;
        ensure!(n != 0, "PTP payload ended before its declared length");
        ensure!(n <= remaining, "PTP payload exceeded its declared length");
        payload.extend_from_slice(&chunk[..n]);
    }

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
        BulkTransport, Clock, CommandCode, ContainerCode, ContainerInfo, ContainerType, Deadline,
        MAX_PTP_CONTAINER_PAYLOAD_BYTES, PTP_BULK_TIMEOUT, PTP_TRANSACTION_TIMEOUT, ResponseCode,
        encode_command_params, read_container, read_container_with_deadline, send_with_transport,
        send_with_transport_and_clock, send_with_transport_until_and_clock,
        validate_bulk_read_geometry, write_container,
    };

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
        read_errors: RefCell<VecDeque<rusb::Error>>,
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
            if let Some(error) = self.read_errors.borrow_mut().pop_front() {
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
                    .push_back(rusb::Error::NoDevice),
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
        }

        assert!(
            failures.is_empty(),
            "PTP transport errors lacked safe operation context or exposed payload:\n{}",
            failures.join("\n")
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

            let (header, payload) = read_container_with_deadline(&transport, 0x81, 1024, &deadline)
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
    fn rejects_bulk_read_buffer_that_is_not_packet_aligned() {
        let error = validate_bulk_read_geometry(1000, 512)
            .expect_err("bulk read buffers must align to endpoint packets");

        assert!(error.to_string().contains("multiple"));
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
    fn keeps_session_reusable_after_well_framed_ptp_error_response() {
        let error_response = container(
            ContainerType::Response,
            ContainerCode::Response(ResponseCode::GeneralError),
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
        let clock = FakeClock::new([start, start, start + Duration::from_secs(2)]);
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
