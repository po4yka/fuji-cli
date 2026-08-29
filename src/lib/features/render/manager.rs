use std::{
    thread::sleep,
    time::{Duration, Instant},
};

use crate::{
    features::base::CameraBase,
    generated::renders::RenderBase,
    ptp::{CommandCode, DevicePropCode, ObjectFormat, ObjectInfo, Ptp},
};
use log::debug;

pub const OUTGOING_OBJECT_HANDLE: [u32; 3] = [0x0, 0x0, 0x0];
pub const INCOMING_OBJECT_HANDLE: [u32; 3] = [u32::MAX, 0x0, 0x0];
const RENDER_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub struct RenderCleanupError {
    rendered_data: Vec<u8>,
    cleanup: anyhow::Error,
}

impl RenderCleanupError {
    pub fn new(rendered_data: Vec<u8>, cleanup: anyhow::Error) -> Self {
        Self {
            rendered_data,
            cleanup,
        }
    }

    pub fn rendered_data(&self) -> &[u8] {
        &self.rendered_data
    }

    pub fn into_rendered_data(self) -> Vec<u8> {
        self.rendered_data
    }

    pub fn cleanup_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self.cleanup.as_ref()
    }
}

impl std::fmt::Display for RenderCleanupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "rendered image was fetched, but camera cleanup failed: {}",
            self.cleanup
        )
    }
}

impl std::error::Error for RenderCleanupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cleanup.as_ref())
    }
}

#[derive(Debug)]
pub struct RenderFetchError {
    fetch: anyhow::Error,
    cleanup: anyhow::Error,
}

impl RenderFetchError {
    pub fn fetch_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self.fetch.as_ref()
    }

    pub fn cleanup_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self.cleanup.as_ref()
    }
}

impl std::fmt::Display for RenderFetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "rendered image fetch failed and camera cleanup also failed: {}; cleanup: {}",
            self.fetch, self.cleanup
        )
    }
}

impl std::error::Error for RenderFetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.fetch.as_ref())
    }
}

trait RenderIo {
    fn start_render(&mut self, draft: bool) -> anyhow::Result<()>;
    fn object_handles(&mut self, deadline: Instant) -> anyhow::Result<Vec<u32>>;
}

impl RenderIo for Ptp {
    fn start_render(&mut self, draft: bool) -> anyhow::Result<()> {
        self.set_prop(DevicePropCode::FujiRawConversionRun, &u16::from(!draft))
    }

    fn object_handles(&mut self, deadline: Instant) -> anyhow::Result<Vec<u32>> {
        let response = self.send_until(
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
}

trait RenderObjectIo {
    fn fetch_object(&mut self, handle: u32) -> anyhow::Result<Vec<u8>>;
    fn delete_object(&mut self, handle: u32) -> anyhow::Result<()>;
}

impl RenderObjectIo for Ptp {
    fn fetch_object(&mut self, handle: u32) -> anyhow::Result<Vec<u8>> {
        self.send(CommandCode::GetObject, &[handle], None)
    }

    fn delete_object(&mut self, handle: u32) -> anyhow::Result<()> {
        self.send(CommandCode::DeleteObject, &[handle], None)?;
        Ok(())
    }
}

fn fetch_rendered_object<I: RenderObjectIo>(io: &mut I, handle: u32) -> anyhow::Result<Vec<u8>> {
    debug!("Fetching rendered image");
    let image = io.fetch_object(handle);
    if image.is_ok() {
        debug!("Fetched rendered image");
    }

    debug!("Cleaning up rendered image on camera");
    let cleanup = io.delete_object(handle);
    if cleanup.is_ok() {
        debug!("Cleaned up rendered image on camera");
    }

    match (image, cleanup) {
        (Ok(rendered_data), Ok(())) => Ok(rendered_data),
        (Ok(rendered_data), Err(cleanup)) => {
            Err(RenderCleanupError::new(rendered_data, cleanup).into())
        }
        (Err(fetch), Ok(())) => Err(fetch),
        (Err(fetch), Err(cleanup)) => Err(RenderFetchError { fetch, cleanup }.into()),
    }
}

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

fn start_and_wait_for_new_rendered_handle<I, S, N>(
    io: &mut I,
    draft: bool,
    sleep_between_polls: S,
    now: N,
    timeout: Duration,
) -> anyhow::Result<u32>
where
    I: RenderIo,
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

// NOTE: Naively assuming that all cameras render in a similar way.
pub trait CameraRenderManager: CameraBase {
    fn send_image(&self, ptp: &mut Ptp, image: &[u8]) -> anyhow::Result<()> {
        debug!("Sending image to camera");
        let object_info = ObjectInfo {
            object_format: ObjectFormat::FujiRAF,
            compressed_size: u32::try_from(image.len())?,
            filename: String::from("FUP_FILE.dat"),
            ..Default::default()
        };
        let object_info = crate::ptp::codec::encode(&object_info)?;

        ptp.send(
            CommandCode::FujiSendObjectInfo,
            &OUTGOING_OBJECT_HANDLE,
            Some(&object_info),
        )?;
        ptp.send(CommandCode::FujiSendObject, &[], Some(image))?;
        debug!("Sent image to camera");

        Ok(())
    }

    fn render_image(&self, ptp: &mut Ptp, draft: bool) -> anyhow::Result<Vec<u8>> {
        debug!("Starting image render");
        let handle = start_and_wait_for_new_rendered_handle(
            ptp,
            draft,
            sleep,
            Instant::now,
            RENDER_TIMEOUT,
        )?;

        fetch_rendered_object(ptp, handle)
    }

    fn render(
        &self,
        ptp: &mut Ptp,
        image: &[u8],
        partial: RenderBase,
        draft: bool,
    ) -> anyhow::Result<Vec<u8>>;
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
        RENDER_TIMEOUT, RenderCleanupError, RenderFetchError, RenderIo, RenderObjectIo,
        fetch_rendered_object, start_and_wait_for_new_rendered_handle, wait_for_rendered_handle,
    };

    #[derive(Debug, PartialEq, Eq)]
    enum RenderIoCall {
        Start,
        Handles,
    }

    struct FakeRenderIo {
        handles: VecDeque<Vec<u32>>,
        calls: Vec<RenderIoCall>,
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
                .ok_or_else(|| anyhow!("test handle queue exhausted"))
        }
    }

    struct FakeRenderObjectIo {
        fetch_result: Option<anyhow::Result<Vec<u8>>>,
        delete_result: Option<anyhow::Result<()>>,
        deleted: Vec<u32>,
    }

    impl RenderObjectIo for FakeRenderObjectIo {
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
    fn attempts_cleanup_when_fetching_the_rendered_object_fails() {
        let mut io = FakeRenderObjectIo {
            fetch_result: Some(Err(anyhow!("simulated fetch failure"))),
            delete_result: Some(Ok(())),
            deleted: Vec::new(),
        };

        let error = fetch_rendered_object(&mut io, 42)
            .expect_err("fetch failure must be returned after cleanup is attempted");

        assert!(error.to_string().contains("simulated fetch failure"));
        assert_eq!(io.deleted, [42]);
    }

    #[test]
    fn cleanup_error_preserves_successfully_fetched_rendered_data() {
        let expected = vec![1, 2, 3, 4];
        let mut io = FakeRenderObjectIo {
            fetch_result: Some(Ok(expected.clone())),
            delete_result: Some(Err(anyhow!("simulated cleanup failure"))),
            deleted: Vec::new(),
        };

        let error = fetch_rendered_object(&mut io, 42)
            .expect_err("cleanup failure must be returned without dropping fetched data");

        assert!(
            error.downcast_ref::<RenderCleanupError>().is_some(),
            "cleanup failure must retain a typed outcome: {error:#}"
        );
        let cleanup = error
            .downcast::<RenderCleanupError>()
            .expect("typed cleanup error was checked above");
        assert_eq!(cleanup.rendered_data(), expected);
        assert_eq!(
            cleanup.cleanup_error().to_string(),
            "simulated cleanup failure"
        );
        assert_eq!(cleanup.into_rendered_data(), expected);
    }

    #[test]
    fn combined_fetch_and_cleanup_error_preserves_both_failures() {
        let mut io = FakeRenderObjectIo {
            fetch_result: Some(Err(anyhow!("simulated fetch failure"))),
            delete_result: Some(Err(anyhow!("simulated cleanup failure"))),
            deleted: Vec::new(),
        };

        let error = fetch_rendered_object(&mut io, 42)
            .expect_err("both lifecycle failures must be returned together");

        assert!(
            error.downcast_ref::<RenderFetchError>().is_some(),
            "both failures must retain a typed outcome: {error:#}"
        );
        let combined = error
            .downcast::<RenderFetchError>()
            .expect("typed fetch error was checked above");
        assert_eq!(
            combined.fetch_error().to_string(),
            "simulated fetch failure"
        );
        assert_eq!(
            combined.cleanup_error().to_string(),
            "simulated cleanup failure"
        );
    }

    #[test]
    fn baselines_handles_before_render_and_selects_only_a_new_handle() {
        let start = Instant::now();
        let mut instants = VecDeque::from([start; 5]);
        let mut io = FakeRenderIo {
            handles: VecDeque::from([vec![7], vec![7, 42]]),
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
