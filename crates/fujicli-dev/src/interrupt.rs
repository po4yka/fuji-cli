use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const CAMERA_STATE_UNKNOWN_EXIT_CODE: i32 = 3;

/// Set from immediately before the mutating probe write through restore
/// verification, so the signal handler can defer the first interrupt.
static IN_CRITICAL_WRITE: AtomicBool = AtomicBool::new(false);

/// Counts interrupts received while the selector is temporarily changed.
static INTERRUPTS_SEEN: AtomicU8 = AtomicU8::new(0);

enum InterruptAction {
    Exit,
    Latch,
    ForceExitUnknown,
}

fn record_interrupt() -> InterruptAction {
    if !IN_CRITICAL_WRITE.load(Ordering::SeqCst) {
        return InterruptAction::Exit;
    }

    if INTERRUPTS_SEEN.fetch_add(1, Ordering::SeqCst) == 0 {
        InterruptAction::Latch
    } else {
        InterruptAction::ForceExitUnknown
    }
}

/// Result of a critical camera sequence.
pub enum CriticalWriteError<E> {
    /// The guarded operation returned its own error.
    Operation(E),
    /// An interrupt was requested while camera state was temporarily changed.
    Interrupted,
}

/// Install the process-wide Ctrl-C handler before dispatching a command.
pub fn install() -> anyhow::Result<()> {
    ctrlc::set_handler(|| match record_interrupt() {
        InterruptAction::Exit => {
            eprintln!("interrupted");
            std::process::exit(130);
        }
        InterruptAction::Latch => {
            eprintln!(
                "interrupt received during the dangerous probe; restoring and verifying the original camera state first (press Ctrl-C again to force-quit; camera state will then be unknown)"
            );
        }
        InterruptAction::ForceExitUnknown => {
            eprintln!(
                "forced quit during the dangerous probe; camera state is unknown. DO NOT RETRY AUTOMATICALLY"
            );
            std::process::exit(CAMERA_STATE_UNKNOWN_EXIT_CODE);
        }
    })?;
    Ok(())
}

struct CriticalWriteGuard;

impl CriticalWriteGuard {
    fn enter() -> Self {
        INTERRUPTS_SEEN.store(0, Ordering::SeqCst);
        IN_CRITICAL_WRITE.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for CriticalWriteGuard {
    fn drop(&mut self) {
        IN_CRITICAL_WRITE.store(false, Ordering::SeqCst);
    }
}

/// Run one camera mutation-and-restore sequence as an interrupt boundary.
pub fn critical_camera_write<T, E>(
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, CriticalWriteError<E>> {
    let guard = CriticalWriteGuard::enter();
    let result = operation();
    drop(guard);

    let interrupted = INTERRUPTS_SEEN.swap(0, Ordering::SeqCst) > 0;
    match result {
        Err(error) => Err(CriticalWriteError::Operation(error)),
        Ok(_) if interrupted => Err(CriticalWriteError::Interrupted),
        Ok(value) => Ok(value),
    }
}

#[cfg(test)]
pub fn simulate_interrupt() {
    assert!(matches!(record_interrupt(), InterruptAction::Latch));
}
