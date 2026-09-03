use fujicli::interrupt::{INTERRUPTED_EXIT_CODE, INTERRUPTS};
use log::warn;

use super::camera_state::{CAMERA_STATE_UNKNOWN_EXIT_CODE, CameraStateUnknown};

fn interrupt_exit_code(during_critical_write: bool) -> i32 {
    if during_critical_write {
        i32::from(CAMERA_STATE_UNKNOWN_EXIT_CODE)
    } else {
        i32::from(INTERRUPTED_EXIT_CODE)
    }
}

/// Install the process-wide Ctrl-C handler. Call exactly once, before
/// dispatching any command.
///
/// The PTP transport marks every transaction as in flight on the shared
/// [`INTERRUPTS`] latch, so an interrupt is never allowed to abandon a bulk
/// transfer mid-stream: a read-only transaction finishes and then fails with
/// `fujicli::interrupt::Interrupted`, while a `critical_camera_write` region
/// runs to completion and reports the interrupt itself.
pub fn install() -> anyhow::Result<()> {
    ctrlc::set_handler(|| {
        if !INTERRUPTS.is_deferring() {
            // No PTP transaction in flight: behave like the default disposition.
            eprintln!("interrupted");
            std::process::exit(interrupt_exit_code(false));
        }
        let seen = INTERRUPTS.record_interrupt();
        let during_critical_write = INTERRUPTS.critical_region_active();
        match (seen, during_critical_write) {
            (0, true) => eprintln!(
                "interrupt received during a camera write; finishing the current PTP operation first (press Ctrl-C again to force-quit; camera state will then be unknown)"
            ),
            (0, false) => eprintln!(
                "interrupt received during a PTP transfer; finishing it first so the camera is not left mid-transfer (press Ctrl-C again to force-quit; the camera may then need to be reconnected)"
            ),
            (_, true) => {
                eprintln!(
                    "forced quit during a camera write; camera state is unknown. DO NOT RETRY AUTOMATICALLY"
                );
                std::process::exit(interrupt_exit_code(true));
            }
            (_, false) => {
                eprintln!(
                    "forced quit during a PTP transfer; disconnect and reconnect the camera before the next command"
                );
                std::process::exit(interrupt_exit_code(false));
            }
        }
    })?;
    Ok(())
}

/// Run `operation` with interrupts latched: a SIGINT arriving while it runs
/// will not terminate the process until `operation` returns, and the
/// transport will not unwind between the transactions it issues. If an
/// interrupt arrived during the run, this logs the standard do-not-retry
/// guidance and returns an error even when `operation` itself succeeded, so
/// the caller's normal error path reports the message and exits non-zero
/// without retrying automatically. `operation`'s own error is returned
/// unchanged when it fails.
pub fn critical_camera_write<T>(
    description: &str,
    operation: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    // The guard clears the region on drop, including on an early `?` return
    // or a panic-driven unwind, so the latch can never be left set.
    let region = INTERRUPTS.enter_critical_region();
    let result = operation();
    drop(region);

    // Always drain the counter, whether `operation` succeeded or failed, so
    // a latched interrupt can never leak into a later critical write.
    let interrupted = INTERRUPTS.take_interrupts() > 0;
    let value = result?;

    if interrupted {
        warn!(
            "{description} completed, but an interrupt was requested during the write; camera state must be verified before any further camera work. DO NOT RETRY AUTOMATICALLY"
        );
        return Err(anyhow::Error::new(CameraStateUnknown).context(format!(
            "{description} completed, but an interrupt was requested; stopping before any further camera work"
        )));
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use fujicli::interrupt::INTERRUPTS;

    use super::{critical_camera_write, interrupt_exit_code};

    #[test]
    fn forced_interrupt_during_camera_write_is_state_unknown() {
        assert_eq!(interrupt_exit_code(false), 130);
        assert_eq!(interrupt_exit_code(true), 3);
    }

    /// Test-only helper: simulate an interrupt having been latched during the
    /// operation, without installing a real signal handler.
    fn simulate_interrupt() {
        INTERRUPTS.record_interrupt();
    }

    #[test]
    fn latch_state_machine() {
        // Phase 1: no interrupt recorded -- the operation's own result passes
        // through untouched.
        let result = critical_camera_write("phase one", || {
            assert!(INTERRUPTS.critical_region_active());
            Ok(42)
        });
        assert_eq!(result.unwrap(), 42);
        assert!(!INTERRUPTS.critical_region_active());
        assert_eq!(INTERRUPTS.take_interrupts(), 0);

        // Phase 2: an interrupt arrives during a successful operation -- the
        // function must still report an error naming the description, and
        // clear the region flag and the interrupt counter.
        let result = critical_camera_write("phase two", || {
            simulate_interrupt();
            Ok(7)
        });
        let error = result.unwrap_err();
        assert!(error.to_string().contains("phase two"));
        assert!(!INTERRUPTS.critical_region_active());
        assert_eq!(INTERRUPTS.take_interrupts(), 0);

        // Phase 3: the operation itself fails, with an interrupt also
        // latched -- the operation's own error must win, and the flag must
        // still be clear afterward.
        let result: anyhow::Result<()> = critical_camera_write("phase three", || {
            simulate_interrupt();
            anyhow::bail!("operation failed")
        });
        let error = result.unwrap_err();
        assert_eq!(error.to_string(), "operation failed");
        assert!(!INTERRUPTS.critical_region_active());
        assert_eq!(INTERRUPTS.take_interrupts(), 0);
    }
}
