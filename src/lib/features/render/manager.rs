use std::{
    thread::sleep,
    time::{Duration, Instant},
};

use crate::{
    features::{
        base::CameraBase,
        outcome::{OutcomeStatus, StateChangeAudit},
    },
    generated::renders::RenderBase,
    ptp::{CommandCode, DevicePropCode, ObjectFormat, ObjectInfo, PtpOperation},
};
use log::debug;

pub(crate) const OUTGOING_OBJECT_HANDLE: [u32; 3] = [0x0, 0x0, 0x0];
const RENDER_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub struct RenderedObject {
    handle: u32,
    data: Vec<u8>,
}

impl RenderedObject {
    fn new(handle: u32, data: Vec<u8>) -> Self {
        Self { handle, data }
    }

    pub fn handle(&self) -> u32 {
        self.handle
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Debug)]
pub struct RenderOutcome {
    rendered: RenderedObject,
    profile_restore_error: Option<anyhow::Error>,
}

impl RenderOutcome {
    pub fn new(rendered: RenderedObject, profile_restore_error: Option<anyhow::Error>) -> Self {
        Self {
            rendered,
            profile_restore_error,
        }
    }

    pub fn rendered(&self) -> &RenderedObject {
        &self.rendered
    }

    pub fn into_parts(self) -> (RenderedObject, Option<anyhow::Error>) {
        (self.rendered, self.profile_restore_error)
    }
}

#[derive(Debug)]
pub struct RenderFailureWithRestoreError {
    render: anyhow::Error,
    restore: anyhow::Error,
}

#[derive(Debug)]
pub struct RenderHandleDiscoveryError {
    observed_handles: Vec<u32>,
    cause: anyhow::Error,
}

impl RenderHandleDiscoveryError {
    fn new(observed_handles: Vec<u32>, cause: anyhow::Error) -> Self {
        Self {
            observed_handles,
            cause,
        }
    }

    pub fn observed_handles(&self) -> &[u32] {
        &self.observed_handles
    }
}

impl std::fmt::Display for RenderHandleDiscoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "RAW render was triggered, but its camera object could not be identified: {}",
            self.cause
        )?;
        if self.observed_handles.is_empty() {
            formatter.write_str("; no new object handle was observed before failure")
        } else {
            write!(
                formatter,
                "; observed new camera object handles [{}] were retained",
                format_handles(&self.observed_handles)
            )
        }
    }
}

impl std::error::Error for RenderHandleDiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

#[derive(Debug)]
pub struct RenderObjectRetentionError {
    handles: Vec<u32>,
    cause: anyhow::Error,
}

impl RenderObjectRetentionError {
    fn new(handles: &[u32], cause: anyhow::Error) -> Self {
        Self {
            handles: handles.to_vec(),
            cause,
        }
    }

    pub fn handles(&self) -> &[u32] {
        &self.handles
    }
}

impl std::fmt::Display for RenderObjectRetentionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "no deletion was attempted for camera object handles [{}]; recover a JPEG with `fujicli image recover HANDLE OUTPUT --target-serial-sha256 SHA256`: {}",
            format_handles(&self.handles),
            self.cause
        )
    }
}

impl std::error::Error for RenderObjectRetentionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

fn format_handles(handles: &[u32]) -> String {
    handles
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

impl std::fmt::Display for RenderFailureWithRestoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "RAW conversion failed and conversion-profile restoration also failed: {}; restore: {}",
            self.render, self.restore
        )
    }
}

impl std::error::Error for RenderFailureWithRestoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.render.as_ref())
    }
}

pub fn combine_render_and_restore(
    render: anyhow::Result<RenderedObject>,
    restore: anyhow::Result<()>,
) -> anyhow::Result<RenderOutcome> {
    match (render, restore) {
        (Ok(rendered), Ok(())) => Ok(RenderOutcome::new(rendered, None)),
        (Ok(rendered), Err(restore)) => Ok(RenderOutcome::new(rendered, Some(restore))),
        (Err(render), Ok(())) => Err(render),
        (Err(render), Err(restore)) => {
            Err(RenderFailureWithRestoreError { render, restore }.into())
        }
    }
}

#[derive(Debug)]
pub struct RenderCleanupError {
    handle: u32,
    profile_restore: Option<anyhow::Error>,
    camera_object_cleanup: Option<anyhow::Error>,
}

#[derive(Debug)]
pub struct RenderSaveError {
    handle: u32,
    save: anyhow::Error,
    profile_restore: Option<anyhow::Error>,
}

impl RenderSaveError {
    pub fn new(handle: u32, save: anyhow::Error, profile_restore: Option<anyhow::Error>) -> Self {
        Self {
            handle,
            save,
            profile_restore,
        }
    }

    pub fn handle(&self) -> u32 {
        self.handle
    }

    pub fn profile_restore_error(
        &self,
    ) -> Option<&(dyn std::error::Error + Send + Sync + 'static)> {
        self.profile_restore.as_deref()
    }
}

impl std::fmt::Display for RenderSaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "saving rendered JPEG failed; camera object {} was retained and can be recovered explicitly: {}",
            self.handle, self.save
        )?;
        if let Some(error) = &self.profile_restore {
            write!(
                formatter,
                "; conversion-profile restoration also failed: {error}"
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for RenderSaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.save.as_ref())
    }
}

impl RenderCleanupError {
    pub fn new(
        handle: u32,
        profile_restore: Option<anyhow::Error>,
        camera_object_cleanup: Option<anyhow::Error>,
    ) -> Self {
        Self {
            handle,
            profile_restore,
            camera_object_cleanup,
        }
    }

    pub fn handle(&self) -> u32 {
        self.handle
    }

    pub fn profile_restore_error(
        &self,
    ) -> Option<&(dyn std::error::Error + Send + Sync + 'static)> {
        self.profile_restore.as_deref()
    }

    pub fn camera_object_cleanup_error(
        &self,
    ) -> Option<&(dyn std::error::Error + Send + Sync + 'static)> {
        self.camera_object_cleanup.as_deref()
    }
}

impl std::fmt::Display for RenderCleanupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "rendered JPEG was saved, but camera cleanup failed for object handle {}",
            self.handle
        )?;
        if let Some(error) = &self.profile_restore {
            write!(formatter, "; conversion-profile restoration: {error}")?;
        }
        if let Some(error) = &self.camera_object_cleanup {
            write!(formatter, "; camera object cleanup: {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RenderCleanupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.profile_restore
            .as_ref()
            .or(self.camera_object_cleanup.as_ref())
            .map(|error| error.as_ref() as &(dyn std::error::Error + 'static))
    }
}

pub fn finish_render_cleanup(
    handle: u32,
    profile_restore: Option<anyhow::Error>,
    camera_object_cleanup: anyhow::Result<()>,
) -> anyhow::Result<()> {
    let camera_object_cleanup = camera_object_cleanup.err();
    if profile_restore.is_none() && camera_object_cleanup.is_none() {
        Ok(())
    } else {
        Err(RenderCleanupError::new(handle, profile_restore, camera_object_cleanup).into())
    }
}

pub(crate) trait RenderIo {
    fn start_render(&mut self, draft: bool) -> anyhow::Result<()>;
    fn object_handles(&mut self, deadline: Instant) -> anyhow::Result<Vec<u32>>;

    fn mark_processing_complete(&mut self) {}
}

pub(crate) trait RawProfileIo {
    fn get_profile_raw(&mut self) -> anyhow::Result<Vec<u8>>;
    fn set_profile_raw(&mut self, value: &[u8]) -> anyhow::Result<()>;
}

pub(crate) trait RenderObjectIo {
    fn object_info(&mut self, handle: u32) -> anyhow::Result<ObjectInfo>;
    fn fetch_object(&mut self, handle: u32) -> anyhow::Result<Vec<u8>>;
    fn delete_object(&mut self, handle: u32) -> anyhow::Result<()>;
}

pub(crate) trait RenderUploadIo {
    fn send_object_info(&mut self, data: &[u8]) -> anyhow::Result<()>;
    fn send_object(&mut self, data: &[u8]) -> anyhow::Result<()>;
}

pub(crate) trait RenderTransport:
    RawProfileIo + RenderIo + RenderObjectIo + RenderUploadIo
{
    fn is_healthy(&self) -> bool;
    fn firmware_capability_profile(
        &self,
    ) -> anyhow::Result<&'static crate::generated::cameras::CameraFirmwareCapabilityProfile>;
    fn validate_raw_conversion_profile(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()>;
    fn validate_raw_conversion_read_fingerprint(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        declared_field_count: u16,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()>;
}

pub(crate) struct AuthorizedRenderIo<'io> {
    authorized: crate::camera::AuthorizedPtp<'io>,
}
impl<'io> AuthorizedRenderIo<'io> {
    pub(crate) fn new(authorized: crate::camera::AuthorizedPtp<'io>) -> Self {
        Self { authorized }
    }
}

impl RawProfileIo for AuthorizedRenderIo<'_> {
    fn get_profile_raw(&mut self) -> anyhow::Result<Vec<u8>> {
        self.authorized
            .get_prop_raw(DevicePropCode::FujiRawConversionProfile)
    }

    fn set_profile_raw(&mut self, value: &[u8]) -> anyhow::Result<()> {
        self.authorized
            .set_prop_raw(DevicePropCode::FujiRawConversionProfile, value)?;
        Ok(())
    }
}

pub(crate) fn snapshot_raw_conversion_profile(
    io: &mut (impl RawProfileIo + ?Sized),
) -> anyhow::Result<Vec<u8>> {
    io.get_profile_raw()
}

pub(crate) fn write_raw_conversion_profile_verified(
    io: &mut (impl RawProfileIo + ?Sized),
    value: &[u8],
) -> anyhow::Result<()> {
    write_profile_verified(io, value)
}

fn write_profile_verified(
    io: &mut (impl RawProfileIo + ?Sized),
    value: &[u8],
) -> anyhow::Result<()> {
    io.set_profile_raw(value)?;
    let readback = io.get_profile_raw()?;
    anyhow::ensure!(
        readback == value,
        "RAW conversion profile readback did not match the requested value"
    );
    Ok(())
}

impl RenderIo for AuthorizedRenderIo<'_> {
    fn start_render(&mut self, draft: bool) -> anyhow::Result<()> {
        self.authorized.set_prop_for_operation(
            PtpOperation::CameraProcessing,
            DevicePropCode::FujiRawConversionRun,
            &u16::from(!draft),
        )?;
        self.authorized.mark_camera_processing_active();
        Ok(())
    }

    fn object_handles(&mut self, deadline: Instant) -> anyhow::Result<Vec<u32>> {
        let response = self.authorized.send_until(
            deadline,
            CommandCode::GetObjectHandles,
            &[u32::MAX, 0, 0],
            None,
        )?;
        Ok(
            crate::ptp::codec::decode_exact::<crate::ptp::codec::PtpArray<u32>>(&response)?
                .into_inner(),
        )
    }

    fn mark_processing_complete(&mut self) {
        self.authorized.mark_camera_processing_complete();
    }
}

impl RenderUploadIo for AuthorizedRenderIo<'_> {
    fn send_object_info(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.authorized.send_mutating_for_operation(
            PtpOperation::CameraProcessing,
            CommandCode::FujiSendObjectInfo,
            &OUTGOING_OBJECT_HANDLE,
            Some(data),
        )?;
        Ok(())
    }

    fn send_object(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.authorized.send_mutating_for_operation(
            PtpOperation::LargeTransfer,
            CommandCode::FujiSendObject,
            &[],
            Some(data),
        )?;
        Ok(())
    }
}

fn send_image_with_io(io: &mut (impl RenderUploadIo + ?Sized), image: &[u8]) -> anyhow::Result<()> {
    crate::features::render::raf::validate_xt5_raf(image)?;
    let object_info = ObjectInfo {
        object_format: ObjectFormat::FujiRAF,
        compressed_size: u32::try_from(image.len())?,
        filename: String::from("FUP_FILE.dat"),
        ..Default::default()
    };
    let object_info = crate::ptp::codec::encode(&object_info)?;
    io.send_object_info(&object_info)?;
    io.send_object(image)
}

impl RenderObjectIo for AuthorizedRenderIo<'_> {
    fn object_info(&mut self, handle: u32) -> anyhow::Result<ObjectInfo> {
        let response = self
            .authorized
            .send(CommandCode::GetObjectInfo, &[handle], None)?;
        Ok(crate::ptp::codec::decode_exact(&response)?)
    }

    fn fetch_object(&mut self, handle: u32) -> anyhow::Result<Vec<u8>> {
        self.authorized.send_for_operation(
            PtpOperation::LargeTransfer,
            CommandCode::GetObject,
            &[handle],
            None,
        )
    }

    fn delete_object(&mut self, handle: u32) -> anyhow::Result<()> {
        self.authorized
            .send_mutating(CommandCode::DeleteObject, &[handle], None)?;
        Ok(())
    }
}

impl RenderTransport for AuthorizedRenderIo<'_> {
    fn is_healthy(&self) -> bool {
        self.authorized.is_healthy()
    }
    fn firmware_capability_profile(
        &self,
    ) -> anyhow::Result<&'static crate::generated::cameras::CameraFirmwareCapabilityProfile> {
        self.authorized.firmware_capability_profile()
    }
    fn validate_raw_conversion_profile(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        self.authorized
            .validate_raw_conversion_profile(profile_code, header_padding, fields, bytes)
    }

    fn validate_raw_conversion_read_fingerprint(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        declared_field_count: u16,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        self.authorized.validate_raw_conversion_read_fingerprint(
            profile_code,
            header_padding,
            declared_field_count,
            fields,
            bytes,
        )
    }
}

fn fetch_unique_rendered_object<I: RenderObjectIo + ?Sized>(
    io: &mut I,
    handles: &[u32],
) -> anyhow::Result<RenderedObject> {
    fetch_unique_rendered_object_inner(io, handles)
        .map_err(|cause| RenderObjectRetentionError::new(handles, cause).into())
}

fn fetch_unique_rendered_object_inner<I: RenderObjectIo + ?Sized>(
    io: &mut I,
    handles: &[u32],
) -> anyhow::Result<RenderedObject> {
    let mut candidates = Vec::new();
    for handle in handles {
        let object_info = io.object_info(*handle)?;
        if object_info.object_format == ObjectFormat::ExifJpeg {
            candidates.push((*handle, object_info));
        }
    }
    anyhow::ensure!(
        !candidates.is_empty(),
        "render produced no EXIF/JPEG object"
    );
    anyhow::ensure!(
        candidates.len() == 1,
        "render produced multiple JPEG objects; refusing an ambiguous fetch"
    );
    let (handle, object_info) = candidates
        .pop()
        .expect("one rendered JPEG candidate was verified above");
    anyhow::ensure!(
        object_info.compressed_size > 0,
        "rendered JPEG ObjectInfo reports an empty object"
    );
    anyhow::ensure!(
        usize::try_from(object_info.compressed_size)?
            <= crate::ptp::MAX_PTP_CONTAINER_PAYLOAD_BYTES,
        "rendered JPEG ObjectInfo exceeds the PTP payload limit"
    );

    let data = fetch_rendered_object(io, handle)?;
    anyhow::ensure!(
        usize::try_from(object_info.compressed_size)? == data.len(),
        "rendered JPEG length does not match GetObjectInfo"
    );
    crate::features::render::validate_jpeg(&data)?;
    Ok(RenderedObject::new(handle, data))
}

fn recover_rendered_object_with_io<I: RenderObjectIo + ?Sized>(
    io: &mut I,
    handle: u32,
) -> anyhow::Result<RenderedObject> {
    fetch_unique_rendered_object(io, &[handle])
}

fn fetch_rendered_object<I: RenderObjectIo + ?Sized>(
    io: &mut I,
    handle: u32,
) -> anyhow::Result<Vec<u8>> {
    debug!("Fetching rendered image");
    let image = io.fetch_object(handle)?;
    debug!("Fetched rendered image");
    Ok(image)
}

fn delete_object_verified<I: RenderIo + RenderObjectIo + ?Sized>(
    io: &mut I,
    handle: u32,
) -> anyhow::Result<StateChangeAudit> {
    io.delete_object(handle)?;
    let deadline = Instant::now()
        .checked_add(RENDER_TIMEOUT)
        .ok_or_else(|| anyhow::anyhow!("render cleanup deadline overflow"))?;
    let handles = io.object_handles(deadline)?;
    anyhow::ensure!(
        !handles.contains(&handle),
        "DeleteObject was accepted by PTP, but object handle {handle} is still present"
    );
    Ok(StateChangeAudit::ptp_accepted().with_semantic(OutcomeStatus::Succeeded))
}

#[cfg(test)]
fn wait_for_rendered_handle<F, S, N>(
    mut fetch_handles: F,
    mut sleep_between_polls: S,
    mut now: N,
    timeout: Duration,
) -> anyhow::Result<u32>
where
    F: FnMut(Instant) -> anyhow::Result<Vec<u32>>,
    S: FnMut(Duration),
    N: FnMut() -> Instant,
{
    let deadline = now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow::anyhow!("render deadline overflow"))?;
    debug!("Waiting for rendered object handles");

    loop {
        let remaining = deadline.saturating_duration_since(now());
        anyhow::ensure!(!remaining.is_zero(), "render deadline exceeded");

        let handles = fetch_handles(deadline)?;
        let remaining = deadline.saturating_duration_since(now());
        anyhow::ensure!(!remaining.is_zero(), "render deadline exceeded");
        if let Some(handle) = handles.first() {
            return Ok(*handle);
        }

        sleep_between_polls(remaining.min(Duration::from_millis(100)));
    }
}

#[cfg(test)]
fn start_and_wait_for_new_rendered_handle<I, S, N>(
    io: &mut I,
    draft: bool,
    sleep_between_polls: S,
    now: N,
    timeout: Duration,
) -> anyhow::Result<u32>
where
    I: RenderIo + ?Sized,
    S: FnMut(Duration),
    N: FnMut() -> Instant,
{
    let mut baseline = None;
    wait_for_rendered_handle(
        |deadline| {
            if baseline.is_none() {
                baseline = Some(io.object_handles(deadline)?);
                io.start_render(draft)?;
            }
            let current = io.object_handles(deadline)?;
            let baseline = baseline
                .as_ref()
                .expect("baseline is initialized before polling");
            Ok(current
                .into_iter()
                .filter(|handle| !baseline.contains(handle))
                .collect())
        },
        sleep_between_polls,
        now,
        timeout,
    )
}

fn start_and_wait_for_stable_new_handles<I, S, N>(
    io: &mut I,
    draft: bool,
    mut sleep_between_polls: S,
    mut now: N,
    timeout: Duration,
) -> anyhow::Result<Vec<u32>>
where
    I: RenderIo + ?Sized,
    S: FnMut(Duration),
    N: FnMut() -> Instant,
{
    let deadline = now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow::anyhow!("render deadline overflow"))?;
    let baseline = io.object_handles(deadline)?;
    io.start_render(draft)
        .map_err(|cause| RenderHandleDiscoveryError::new(Vec::new(), cause))?;

    let mut previous: Option<Vec<u32>> = None;
    let mut busy_backoff = Duration::from_millis(100);
    loop {
        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() {
            return Err(RenderHandleDiscoveryError::new(
                previous.unwrap_or_default(),
                anyhow::anyhow!("render deadline exceeded"),
            )
            .into());
        }

        let current = match io.object_handles(deadline) {
            Ok(current) => current,
            Err(cause) if is_device_busy(&cause) => {
                let remaining = deadline.saturating_duration_since(now());
                if remaining.is_zero() {
                    return Err(RenderHandleDiscoveryError::new(
                        previous.unwrap_or_default(),
                        anyhow::anyhow!("render deadline exceeded while camera remained busy"),
                    )
                    .into());
                }
                sleep_between_polls(remaining.min(busy_backoff));
                busy_backoff = busy_backoff.saturating_mul(2).min(Duration::from_secs(1));
                continue;
            }
            Err(cause) => {
                return Err(
                    RenderHandleDiscoveryError::new(previous.unwrap_or_default(), cause).into(),
                );
            }
        };
        busy_backoff = Duration::from_millis(100);
        let mut delta = current
            .into_iter()
            .filter(|handle| !baseline.contains(handle))
            .collect::<Vec<_>>();
        delta.sort_unstable();
        delta.dedup();

        if !delta.is_empty() && previous.as_ref() == Some(&delta) {
            io.mark_processing_complete();
            return Ok(delta);
        }
        previous = Some(delta);

        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() {
            return Err(RenderHandleDiscoveryError::new(
                previous.unwrap_or_default(),
                anyhow::anyhow!("render deadline exceeded"),
            )
            .into());
        }
        sleep_between_polls(remaining.min(Duration::from_millis(100)));
    }
}

fn is_device_busy(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<crate::ptp::error::Error>(),
            Some(crate::ptp::error::Error::Response(code))
                if *code == u16::from(crate::ptp::ResponseCode::DeviceBusy)
        )
    })
}

// NOTE: Naively assuming that all cameras render in a similar way.
pub(crate) trait CameraRenderManager: CameraBase {
    fn send_image(&self, io: &mut dyn RenderTransport, image: &[u8]) -> anyhow::Result<()> {
        debug!("Sending image to camera");
        send_image_with_io(io, image)?;
        debug!("Sent image to camera");

        Ok(())
    }

    fn render_object(
        &self,
        io: &mut dyn RenderTransport,
        draft: bool,
    ) -> anyhow::Result<RenderedObject> {
        debug!("Starting image render");
        let handles =
            start_and_wait_for_stable_new_handles(io, draft, sleep, Instant::now, RENDER_TIMEOUT)?;

        fetch_unique_rendered_object(io, &handles)
    }

    fn recover_rendered_object(
        &self,
        io: &mut dyn RenderTransport,
        handle: u32,
    ) -> anyhow::Result<RenderedObject> {
        recover_rendered_object_with_io(io, handle)
    }

    fn cleanup_rendered_object(
        &self,
        io: &mut dyn RenderTransport,
        handle: u32,
    ) -> anyhow::Result<StateChangeAudit> {
        debug!("Cleaning up rendered image on camera");
        let audit = delete_object_verified(io, handle)?;
        debug!("Cleaned up rendered image on camera");
        Ok(audit)
    }

    fn render(
        &self,
        io: &mut dyn RenderTransport,
        image: &[u8],
        partial: RenderBase,
        draft: bool,
    ) -> anyhow::Result<RenderOutcome>;
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Mutex, Once},
        thread::{self, ThreadId},
        time::Instant,
    };

    use anyhow::anyhow;
    use log::{Level, LevelFilter, Log, Metadata, Record};

    use super::{
        RENDER_TIMEOUT, RenderIo, RenderObjectIo, RenderUploadIo, combine_render_and_restore,
        delete_object_verified, fetch_rendered_object, fetch_unique_rendered_object,
        recover_rendered_object_with_io, send_image_with_io,
        start_and_wait_for_new_rendered_handle, start_and_wait_for_stable_new_handles,
        wait_for_rendered_handle, write_profile_verified,
    };

    fn valid_jpeg() -> Vec<u8> {
        vec![
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x01, 0x00, 0x01, 0x03, 0x01, 0x11,
            0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xda, 0x00, 0x0c, 0x03, 0x01, 0x00,
            0x02, 0x11, 0x03, 0x11, 0x00, 0x3f, 0x00, 0x00, 0xff, 0xd9,
        ]
    }

    #[derive(Debug, PartialEq, Eq)]
    enum RenderIoCall {
        Start,
        Handles,
        MarkProcessingComplete,
    }

    struct FakeRenderIo {
        handles: VecDeque<anyhow::Result<Vec<u32>>>,
        calls: Vec<RenderIoCall>,
    }

    struct FakeRawProfileIo {
        readback: Vec<u8>,
        writes: Vec<Vec<u8>>,
    }

    #[derive(Default)]
    struct FakeRenderUploadIo {
        calls: Vec<&'static str>,
    }

    impl RenderUploadIo for FakeRenderUploadIo {
        fn send_object_info(&mut self, _data: &[u8]) -> anyhow::Result<()> {
            self.calls.push("object info");
            Ok(())
        }

        fn send_object(&mut self, _data: &[u8]) -> anyhow::Result<()> {
            self.calls.push("object data");
            Ok(())
        }
    }

    impl super::RawProfileIo for FakeRawProfileIo {
        fn get_profile_raw(&mut self) -> anyhow::Result<Vec<u8>> {
            Ok(self.readback.clone())
        }

        fn set_profile_raw(&mut self, value: &[u8]) -> anyhow::Result<()> {
            self.writes.push(value.to_vec());
            Ok(())
        }
    }

    impl RenderIo for FakeRenderIo {
        fn start_render(&mut self, _draft: bool) -> anyhow::Result<()> {
            self.calls.push(RenderIoCall::Start);
            Ok(())
        }

        fn object_handles(&mut self, _deadline: Instant) -> anyhow::Result<Vec<u32>> {
            self.calls.push(RenderIoCall::Handles);
            self.handles
                .pop_front()
                .ok_or_else(|| anyhow!("test handle queue exhausted"))?
        }

        fn mark_processing_complete(&mut self) {
            self.calls.push(RenderIoCall::MarkProcessingComplete);
        }
    }

    struct FakeRenderObjectIo {
        object_infos: VecDeque<anyhow::Result<crate::ptp::ObjectInfo>>,
        fetch_result: Option<anyhow::Result<Vec<u8>>>,
        delete_result: Option<anyhow::Result<()>>,
        deleted: Vec<u32>,
    }

    #[derive(Default)]
    struct FakeCleanupIo {
        deleted: Vec<u32>,
        handle_readbacks: usize,
    }

    impl RenderObjectIo for FakeCleanupIo {
        fn object_info(&mut self, _handle: u32) -> anyhow::Result<crate::ptp::ObjectInfo> {
            unreachable!("cleanup verification does not inspect ObjectInfo")
        }

        fn fetch_object(&mut self, _handle: u32) -> anyhow::Result<Vec<u8>> {
            unreachable!("cleanup verification does not fetch object data")
        }

        fn delete_object(&mut self, handle: u32) -> anyhow::Result<()> {
            self.deleted.push(handle);
            Ok(())
        }
    }

    impl RenderIo for FakeCleanupIo {
        fn start_render(&mut self, _draft: bool) -> anyhow::Result<()> {
            unreachable!("cleanup verification does not start rendering")
        }

        fn object_handles(&mut self, _deadline: Instant) -> anyhow::Result<Vec<u32>> {
            self.handle_readbacks += 1;
            Ok(vec![42])
        }
    }

    impl RenderObjectIo for FakeRenderObjectIo {
        fn object_info(&mut self, _handle: u32) -> anyhow::Result<crate::ptp::ObjectInfo> {
            self.object_infos
                .pop_front()
                .ok_or_else(|| anyhow!("test ObjectInfo queue exhausted"))?
        }

        fn fetch_object(&mut self, _handle: u32) -> anyhow::Result<Vec<u8>> {
            self.fetch_result
                .take()
                .expect("test fetch result exhausted")
        }

        fn delete_object(&mut self, handle: u32) -> anyhow::Result<()> {
            self.deleted.push(handle);
            self.delete_result
                .take()
                .expect("test delete result exhausted")
        }
    }

    struct CaptureState {
        records: Vec<(Level, String)>,
        thread_id: Option<ThreadId>,
    }

    struct CapturingLogger {
        state: Mutex<CaptureState>,
    }

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() == Level::Debug
                && metadata.target().ends_with("features::render::manager")
        }

        fn log(&self, record: &Record<'_>) {
            if !self.enabled(record.metadata()) {
                return;
            }
            let mut state = self
                .state
                .lock()
                .expect("captured render logs must remain accessible");
            if state.thread_id.as_ref() == Some(&thread::current().id()) {
                state
                    .records
                    .push((record.level(), record.args().to_string()));
            }
        }

        fn flush(&self) {}
    }

    static LOGGER: CapturingLogger = CapturingLogger {
        state: Mutex::new(CaptureState {
            records: Vec::new(),
            thread_id: None,
        }),
    };
    static LOGGER_INIT: Once = Once::new();
    static LOGGER_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn retains_camera_object_when_fetching_the_rendered_object_fails() {
        let mut io = FakeRenderObjectIo {
            object_infos: VecDeque::new(),
            fetch_result: Some(Err(anyhow!("simulated fetch failure"))),
            delete_result: Some(Ok(())),
            deleted: Vec::new(),
        };

        let error = fetch_rendered_object(&mut io, 42)
            .expect_err("fetch failure must be returned without deleting the camera object");

        assert!(error.to_string().contains("simulated fetch failure"));
        assert!(io.deleted.is_empty());
    }

    #[test]
    fn delete_acceptance_is_not_success_while_handle_remains_present() {
        let mut io = FakeCleanupIo::default();

        let result = delete_object_verified(&mut io, 42);

        assert_eq!(io.deleted, [42]);
        assert!(
            result.is_err(),
            "DeleteObject PTP OK must not be verified success while handle 42 remains present"
        );
        assert!(
            io.handle_readbacks > 0,
            "cleanup must verify absence through GetObjectHandles"
        );
    }

    #[test]
    fn fetch_failure_reports_the_owned_render_handle() {
        let mut io = FakeRenderObjectIo {
            object_infos: VecDeque::from([Ok(crate::ptp::ObjectInfo {
                object_format: crate::ptp::ObjectFormat::ExifJpeg,
                compressed_size: 100,
                ..Default::default()
            })]),
            fetch_result: Some(Err(anyhow!("simulated fetch failure"))),
            delete_result: Some(Ok(())),
            deleted: Vec::new(),
        };

        let error = fetch_unique_rendered_object(&mut io, &[42])
            .expect_err("failed fetch must expose the retained object handle");
        let error = error
            .downcast_ref::<super::RenderObjectRetentionError>()
            .expect("post-trigger fetch failure must retain typed handles");

        assert_eq!(error.handles(), &[42]);
        assert!(io.deleted.is_empty());
    }

    #[test]
    fn rejects_ambiguous_multiple_new_jpeg_objects_without_fetching() {
        let jpeg_info = crate::ptp::ObjectInfo {
            object_format: crate::ptp::ObjectFormat::ExifJpeg,
            compressed_size: 6,
            ..Default::default()
        };
        let mut io = FakeRenderObjectIo {
            object_infos: VecDeque::from([Ok(jpeg_info.clone()), Ok(jpeg_info)]),
            fetch_result: Some(Ok(vec![0xff, 0xd8, 0xff, 0xda, 0xff, 0xd9])),
            delete_result: Some(Ok(())),
            deleted: Vec::new(),
        };

        let error = fetch_unique_rendered_object(&mut io, &[42, 43])
            .expect_err("multiple JPEG candidates must remain ambiguous");

        assert!(error.to_string().contains("multiple JPEG"));
        assert!(
            error.to_string().contains("42, 43"),
            "every retained candidate handle must be actionable: {error:#}"
        );
        assert!(
            io.fetch_result.is_some(),
            "ambiguous objects must not be fetched"
        );
        assert!(io.deleted.is_empty());
    }

    #[test]
    fn retains_camera_object_when_fetched_length_disagrees_with_object_info() {
        let mut io = FakeRenderObjectIo {
            object_infos: VecDeque::from([Ok(crate::ptp::ObjectInfo {
                object_format: crate::ptp::ObjectFormat::ExifJpeg,
                compressed_size: 7,
                ..Default::default()
            })]),
            fetch_result: Some(Ok(vec![0xff, 0xd8, 0xff, 0xda, 0xff, 0xd9])),
            delete_result: Some(Ok(())),
            deleted: Vec::new(),
        };

        let error = fetch_unique_rendered_object(&mut io, &[42])
            .expect_err("truncated fetched data must be rejected");

        assert!(error.to_string().contains("does not match GetObjectInfo"));
        assert_eq!(
            error
                .downcast_ref::<super::RenderObjectRetentionError>()
                .expect("post-trigger object failure must retain typed handles")
                .handles(),
            &[42]
        );
        assert!(io.deleted.is_empty());
    }

    #[test]
    fn object_info_failure_reports_all_stable_candidate_handles() {
        let mut io = FakeRenderObjectIo {
            object_infos: VecDeque::from([Err(anyhow!("simulated GetObjectInfo failure"))]),
            fetch_result: None,
            delete_result: Some(Ok(())),
            deleted: Vec::new(),
        };

        let error = fetch_unique_rendered_object(&mut io, &[42, 43])
            .expect_err("ObjectInfo failure must keep every stable candidate actionable");
        let error = error
            .downcast_ref::<super::RenderObjectRetentionError>()
            .expect("post-trigger object failure must retain typed handles");

        assert_eq!(error.handles(), &[42, 43]);
        assert!(io.deleted.is_empty());
    }

    #[test]
    fn retains_camera_object_when_fetched_jpeg_is_structurally_invalid() {
        let mut io = FakeRenderObjectIo {
            object_infos: VecDeque::from([Ok(crate::ptp::ObjectInfo {
                object_format: crate::ptp::ObjectFormat::ExifJpeg,
                compressed_size: 6,
                ..Default::default()
            })]),
            fetch_result: Some(Ok(vec![0xff, 0xd8, 0xff, 0xda, 0xff, 0xd9])),
            delete_result: Some(Ok(())),
            deleted: Vec::new(),
        };

        let error = fetch_unique_rendered_object(&mut io, &[42])
            .expect_err("structurally invalid JPEG must be rejected");

        assert!(error.to_string().contains("JPEG"));
        assert_eq!(
            error
                .downcast_ref::<super::RenderObjectRetentionError>()
                .expect("JPEG validation failure must retain typed handles")
                .handles(),
            &[42]
        );
        assert!(io.deleted.is_empty());
    }

    #[test]
    fn explicit_handle_recovery_fetches_validated_jpeg_without_deleting_it() {
        let jpeg = valid_jpeg();
        let mut io = FakeRenderObjectIo {
            object_infos: VecDeque::from([Ok(crate::ptp::ObjectInfo {
                object_format: crate::ptp::ObjectFormat::ExifJpeg,
                compressed_size: u32::try_from(jpeg.len()).expect("fixture fits u32"),
                ..Default::default()
            })]),
            fetch_result: Some(Ok(jpeg.clone())),
            delete_result: Some(Ok(())),
            deleted: Vec::new(),
        };

        let recovered = recover_rendered_object_with_io(&mut io, 42)
            .expect("valid JPEG should be recoverable by explicit handle");

        assert_eq!(recovered.handle(), 42);
        assert_eq!(recovered.data(), jpeg);
        assert!(io.deleted.is_empty());
    }

    #[test]
    fn baselines_handles_before_render_and_selects_only_a_new_handle() {
        let start = Instant::now();
        let mut instants = VecDeque::from([start; 5]);
        let mut io = FakeRenderIo {
            handles: VecDeque::from([Ok(vec![7]), Ok(vec![7, 42])]),
            calls: Vec::new(),
        };

        let handle = start_and_wait_for_new_rendered_handle(
            &mut io,
            false,
            |_| {},
            || instants.pop_front().expect("test clock exhausted"),
            RENDER_TIMEOUT,
        )
        .expect("a new rendered handle should be selected");

        assert_eq!(handle, 42);
        assert_eq!(
            io.calls,
            [
                RenderIoCall::Handles,
                RenderIoCall::Start,
                RenderIoCall::Handles,
            ]
        );
    }

    #[test]
    fn waits_for_two_stable_polls_before_finalizing_new_handles() {
        let start = Instant::now();
        let mut instants = VecDeque::from([start; 12]);
        let mut io = FakeRenderIo {
            handles: VecDeque::from([
                Ok(vec![7]),
                Ok(vec![7, 42]),
                Ok(vec![7, 42, 43]),
                Ok(vec![7, 42, 43]),
            ]),
            calls: Vec::new(),
        };

        let handles = start_and_wait_for_stable_new_handles(
            &mut io,
            false,
            |_| {},
            || instants.pop_front().expect("test clock exhausted"),
            RENDER_TIMEOUT,
        )
        .expect("stable rendered handles should be returned");

        assert_eq!(handles, [42, 43]);
    }

    #[test]
    fn marks_processing_complete_once_after_two_stable_non_empty_handle_polls() {
        let start = Instant::now();
        let mut instants = VecDeque::from([start; 12]);
        let mut io = FakeRenderIo {
            handles: VecDeque::from([Ok(vec![7]), Ok(vec![7, 42]), Ok(vec![7, 42])]),
            calls: Vec::new(),
        };

        let handles = start_and_wait_for_stable_new_handles(
            &mut io,
            false,
            |_| {},
            || instants.pop_front().expect("test clock exhausted"),
            RENDER_TIMEOUT,
        )
        .expect("stable rendered handles should mark camera processing complete");

        assert_eq!(handles, [42]);
        assert_eq!(
            io.calls,
            [
                RenderIoCall::Handles,
                RenderIoCall::Start,
                RenderIoCall::Handles,
                RenderIoCall::Handles,
                RenderIoCall::MarkProcessingComplete,
            ]
        );
    }

    #[test]
    fn well_framed_device_busy_during_handle_polling_is_retried_within_deadline() {
        let start = Instant::now();
        let mut instants = VecDeque::from([start; 12]);
        let mut io = FakeRenderIo {
            handles: VecDeque::from([
                Ok(vec![7]),
                Err(anyhow!(crate::ptp::error::Error::Response(
                    crate::ptp::ResponseCode::DeviceBusy.into(),
                ))),
                Ok(vec![7, 42]),
                Ok(vec![7, 42]),
            ]),
            calls: Vec::new(),
        };

        let handles = start_and_wait_for_stable_new_handles(
            &mut io,
            false,
            |_| {},
            || instants.pop_front().expect("test clock exhausted"),
            RENDER_TIMEOUT,
        )
        .expect("well-framed DeviceBusy should be retried within the polling deadline");

        assert_eq!(handles, [42]);
        assert_eq!(
            io.calls,
            [
                RenderIoCall::Handles,
                RenderIoCall::Start,
                RenderIoCall::Handles,
                RenderIoCall::Handles,
                RenderIoCall::Handles,
                RenderIoCall::MarkProcessingComplete,
            ]
        );
    }

    #[test]
    fn repeated_device_busy_uses_bounded_increasing_backoff_within_render_deadline() {
        let start = Instant::now();
        let timeout = std::time::Duration::from_millis(500);
        let mut instants = VecDeque::from([
            start,
            start,
            start,
            start + std::time::Duration::from_millis(100),
            start + std::time::Duration::from_millis(100),
            start + std::time::Duration::from_millis(300),
            start + std::time::Duration::from_millis(300),
            start + std::time::Duration::from_millis(400),
        ]);
        let mut sleeps = Vec::new();
        let mut io = FakeRenderIo {
            handles: VecDeque::from([
                Ok(vec![7]),
                Err(anyhow!(crate::ptp::error::Error::Response(
                    crate::ptp::ResponseCode::DeviceBusy.into(),
                ))),
                Err(anyhow!(crate::ptp::error::Error::Response(
                    crate::ptp::ResponseCode::DeviceBusy.into(),
                ))),
                Ok(vec![7, 42]),
                Ok(vec![7, 42]),
            ]),
            calls: Vec::new(),
        };

        let handles = start_and_wait_for_stable_new_handles(
            &mut io,
            false,
            |duration| sleeps.push(duration),
            || instants.pop_front().expect("test clock exhausted"),
            timeout,
        )
        .expect("bounded DeviceBusy retries should still discover the stable rendered handle");

        assert_eq!(handles, [42]);
        assert_eq!(
            sleeps,
            [
                std::time::Duration::from_millis(100),
                std::time::Duration::from_millis(200),
                std::time::Duration::from_millis(100),
            ]
        );
        assert!(sleeps.into_iter().sum::<std::time::Duration>() <= timeout);
    }

    #[test]
    fn polling_failure_reports_every_observed_new_handle() {
        let start = Instant::now();
        let mut instants = VecDeque::from([start; 8]);
        let mut io = FakeRenderIo {
            handles: VecDeque::from([
                Ok(vec![7]),
                Ok(vec![7, 42, 43]),
                Err(anyhow!("simulated USB polling failure")),
            ]),
            calls: Vec::new(),
        };

        let error = start_and_wait_for_stable_new_handles(
            &mut io,
            false,
            |_| {},
            || instants.pop_front().expect("test clock exhausted"),
            RENDER_TIMEOUT,
        )
        .expect_err("polling failure must retain observed recovery handles");
        let error = error
            .downcast_ref::<super::RenderHandleDiscoveryError>()
            .expect("handle discovery failure must remain typed");

        assert_eq!(error.observed_handles(), &[42, 43]);
    }

    #[test]
    fn profile_write_rejects_a_readback_mismatch() {
        let mut io = FakeRawProfileIo {
            readback: vec![9, 9],
            writes: Vec::new(),
        };

        let error = write_profile_verified(&mut io, &[1, 2])
            .expect_err("profile write must require exact readback");

        assert_eq!(io.writes, [vec![1, 2]]);
        assert!(error.to_string().contains("readback"));
    }

    #[test]
    fn render_and_profile_restore_failures_are_both_preserved() {
        let error = combine_render_and_restore(
            Err(anyhow!("simulated render failure")),
            Err(anyhow!("simulated profile restore failure")),
        )
        .expect_err("both failures must keep the command unsuccessful");
        let error = error
            .downcast_ref::<super::RenderFailureWithRestoreError>()
            .expect("combined render/restore failure must remain typed");

        assert!(error.to_string().contains("simulated render failure"));
        assert!(
            error
                .to_string()
                .contains("simulated profile restore failure")
        );
    }

    #[test]
    fn invalid_raf_is_rejected_before_upload_metadata() {
        let mut io = FakeRenderUploadIo::default();

        let error = send_image_with_io(&mut io, b"not a RAF")
            .expect_err("invalid RAF must be rejected before upload");

        assert!(error.to_string().contains("RAF"));
        assert!(io.calls.is_empty());
    }

    #[test]
    fn stops_waiting_for_rendered_object_at_absolute_deadline() {
        let start = Instant::now();
        let mut instants = VecDeque::from([start, start, start + RENDER_TIMEOUT]);
        let mut fetches = VecDeque::from([Ok(Vec::new())]);

        let error = wait_for_rendered_handle(
            |_| {
                fetches
                    .pop_front()
                    .unwrap_or_else(|| Err(anyhow!("test fetch queue exhausted")))
            },
            |_| {},
            || instants.pop_front().expect("test clock exhausted"),
            RENDER_TIMEOUT,
        )
        .unwrap_err();

        assert!(error.to_string().contains("render deadline exceeded"));
    }

    #[test]
    fn passes_one_absolute_deadline_to_every_fetch() {
        let start = Instant::now();
        let expected_deadline = start + RENDER_TIMEOUT;
        let mut instants = VecDeque::from([start, start, start, start, start]);
        let mut deadlines = Vec::new();

        let handle = wait_for_rendered_handle(
            |deadline| {
                deadlines.push(deadline);
                Ok(if deadlines.len() == 2 {
                    vec![42]
                } else {
                    vec![]
                })
            },
            |_| {},
            || instants.pop_front().expect("test clock exhausted"),
            RENDER_TIMEOUT,
        )
        .unwrap();

        assert_eq!(handle, 42);
        assert_eq!(deadlines, vec![expected_deadline, expected_deadline]);
    }

    #[test]
    fn multiple_polls_emit_one_bounded_handle_lifecycle_event() {
        let _test_lock = LOGGER_TEST_LOCK
            .lock()
            .expect("render logger tests must run serially");
        LOGGER_INIT.call_once(|| {
            log::set_logger(&LOGGER).expect("render test logger must be installed once");
            log::set_max_level(LevelFilter::Debug);
        });
        {
            let mut state = LOGGER
                .state
                .lock()
                .expect("captured render logs must remain accessible");
            state.records.clear();
            state.thread_id = Some(thread::current().id());
        }

        let start = Instant::now();
        let mut instants = VecDeque::from([start; 7]);
        let mut polls = 0;
        let handle = wait_for_rendered_handle(
            |_| {
                polls += 1;
                Ok(if polls == 3 { vec![424_242] } else { vec![] })
            },
            |_| {},
            || instants.pop_front().expect("test clock exhausted"),
            RENDER_TIMEOUT,
        )
        .unwrap();
        let records = {
            let mut state = LOGGER
                .state
                .lock()
                .expect("captured render logs must remain accessible");
            state.thread_id = None;
            std::mem::take(&mut state.records)
        };

        assert_eq!(handle, 424_242);
        assert_eq!(polls, 3);
        assert!(
            records.len() == 1
                && records.first().is_some_and(|(level, message)| {
                    *level == Level::Debug
                        && matches!(
                            message.as_str(),
                            "Waiting for rendered object handles"
                                | "Received rendered object handles"
                        )
                        && message.len() <= 80
                        && !message.contains("424242")
                }),
            "expected one bounded, identifier-free DEBUG handle lifecycle event, got {records:?}"
        );
    }
}
