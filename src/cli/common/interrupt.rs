use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use log::warn;

use super::camera_state::CameraStateUnknown;

/// Set while a `critical_camera_write` operation is in flight. The Ctrl-C
/// handler consults this to decide whether to latch the interrupt or exit
/// immediately with the default disposition.
static IN_CRITICAL_WRITE: AtomicBool = AtomicBool::new(false);

/// Counts interrupts received while `IN_CRITICAL_WRITE` was set. A non-zero
/// value after a critical write completes means the caller must report the
/// do-not-retry guidance instead of treating the write as a clean success.
static INTERRUPTS_SEEN: AtomicU8 = AtomicU8::new(0);

/// Install the process-wide Ctrl-C handler. Call exactly once, before
/// dispatching any command.
pub fn install() -> anyhow::Result<()> {
    ctrlc::set_handler(|| {
        if !IN_CRITICAL_WRITE.load(Ordering::SeqCst) {
            // Not inside a camera write: behave like the default disposition.
            eprintln!("interrupted");
            std::process::exit(130);
        }
        let seen = INTERRUPTS_SEEN.fetch_add(1, Ordering::SeqCst);
        if seen == 0 {
            eprintln!(
                "interrupt received during a camera write; finishing the current PTP operation first (press Ctrl-C again to force-quit; camera state will then be unknown)"
            );
        } else {
            eprintln!(
                "forced quit during a camera write; camera state is unknown. DO NOT RETRY AUTOMATICALLY"
            );
            std::process::exit(130);
        }
    })?;
    Ok(())
}

/// RAII guard that clears `IN_CRITICAL_WRITE` on drop, including on an early
/// `?` return or a panic-driven unwind, so the latch can never be left set
/// past the operation it was guarding.
struct CriticalWriteGuard;

impl CriticalWriteGuard {
    fn enter() -> Self {
        IN_CRITICAL_WRITE.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for CriticalWriteGuard {
    fn drop(&mut self) {
        IN_CRITICAL_WRITE.store(false, Ordering::SeqCst);
    }
}

/// Run `operation` with interrupts latched: a SIGINT arriving while it runs
/// will not terminate the process until `operation` returns. If an interrupt
/// arrived during the run, this logs the standard do-not-retry guidance and
/// returns an error even when `operation` itself succeeded, so the caller's
/// normal error path reports the message and exits non-zero without
/// retrying automatically. `operation`'s own error is returned unchanged
/// when it fails.
pub fn critical_camera_write<T>(
    description: &str,
    operation: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let guard = CriticalWriteGuard::enter();
    let result = operation();
    drop(guard);

    // Always drain the counter, whether `operation` succeeded or failed, so
    // a latched interrupt can never leak into a later critical write.
    let interrupted = INTERRUPTS_SEEN.swap(0, Ordering::SeqCst) > 0;
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
    use std::sync::atomic::Ordering;

    use super::{INTERRUPTS_SEEN, critical_camera_write};

    /// Test-only helper: simulate an interrupt having been latched during the
    /// operation, without installing a real signal handler.
    fn simulate_interrupt() {
        INTERRUPTS_SEEN.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn latch_state_machine() {
        // Phase 1: no interrupt recorded -- the operation's own result passes
        // through untouched.
        let result = critical_camera_write("phase one", || Ok(42));
        assert_eq!(result.unwrap(), 42);
        assert!(!super::IN_CRITICAL_WRITE.load(Ordering::SeqCst));
        assert_eq!(INTERRUPTS_SEEN.load(Ordering::SeqCst), 0);

        // Phase 2: an interrupt arrives during a successful operation -- the
        // function must still report an error naming the description, and
        // clear the in-critical flag and the interrupt counter.
        let result = critical_camera_write("phase two", || {
            simulate_interrupt();
            Ok(7)
        });
        let error = result.unwrap_err();
        assert!(error.to_string().contains("phase two"));
        assert!(!super::IN_CRITICAL_WRITE.load(Ordering::SeqCst));
        assert_eq!(INTERRUPTS_SEEN.load(Ordering::SeqCst), 0);

        // Phase 3: the operation itself fails, with an interrupt also
        // latched -- the operation's own error must win, and the flag must
        // still be clear afterward.
        let result: anyhow::Result<()> = critical_camera_write("phase three", || {
            simulate_interrupt();
            anyhow::bail!("operation failed")
        });
        let error = result.unwrap_err();
        assert_eq!(error.to_string(), "operation failed");
        assert!(!super::IN_CRITICAL_WRITE.load(Ordering::SeqCst));
        assert_eq!(INTERRUPTS_SEEN.load(Ordering::SeqCst), 0);
    }
}
