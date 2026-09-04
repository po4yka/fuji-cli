//! Process-wide interrupt latch shared by the PTP transport and the CLI.
//!
//! A bulk transfer abandoned mid-transaction leaves the camera with an
//! unfinished USB pipe. On the X-T5 that required a physical cable
//! reconnection (see `docs/internals/x-t5-device-audit-2026-08-31.md`). The
//! transport therefore marks every PTP transaction as in flight, and a signal
//! handler that finds one in flight records the interrupt instead of exiting.
//! The transport honours the recorded interrupt as soon as the transaction
//! completes, unless a critical region (a multi-transaction camera write)
//! asked to see the whole sequence through first.

use std::{
    fmt,
    sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
};

/// Exit status for an interrupt honoured outside a camera write.
pub const INTERRUPTED_EXIT_CODE: u8 = 130;

/// The process-wide latch. The CLI signal handler and the transport share it.
pub static INTERRUPTS: InterruptLatch = InterruptLatch::new();

/// Marker attached to an error chain when a recorded interrupt was honoured
/// after the in-flight PTP transaction completed.
#[derive(Debug)]
pub struct Interrupted {
    /// A state-changing command was already sent earlier in this process, so
    /// the interrupt leaves the camera state unknown and must map to the
    /// unknown-state exit status rather than a plain interruption.
    pub after_camera_write: bool,
}

impl fmt::Display for Interrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "interrupted by Ctrl-C; stopped after the in-flight PTP transaction completed",
        )?;
        if self.after_camera_write {
            f.write_str(
                "; a camera write was already sent, so camera state is unknown. DO NOT RETRY AUTOMATICALLY",
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for Interrupted {}

pub struct InterruptLatch {
    transaction_in_flight: AtomicBool,
    /// Depth of nested critical regions; regions nest, so a counter rather
    /// than a flag, or an inner region's drop would end the outer one.
    critical_region_depth: AtomicUsize,
    camera_write_sent: AtomicBool,
    interrupts_seen: AtomicU8,
}

/// Marks a PTP transaction as in flight until dropped.
#[must_use = "the transaction is only in flight while the guard is alive"]
pub struct TransactionGuard<'a> {
    latch: &'a InterruptLatch,
}

/// Marks a multi-transaction camera write until dropped. Interrupts recorded
/// while it is alive stay pending for the region owner instead of being
/// honoured by the transport.
#[must_use = "the critical region only lasts while the guard is alive"]
pub struct CriticalRegionGuard<'a> {
    latch: &'a InterruptLatch,
}

impl InterruptLatch {
    pub const fn new() -> Self {
        Self {
            transaction_in_flight: AtomicBool::new(false),
            critical_region_depth: AtomicUsize::new(0),
            camera_write_sent: AtomicBool::new(false),
            interrupts_seen: AtomicU8::new(0),
        }
    }

    /// Records that a state-changing command is being sent. The flag is
    /// sticky for the rest of the process: every later interrupt, including
    /// one during the verification that follows a restore, must be reported
    /// as an unknown camera state rather than a plain interruption.
    pub fn mark_camera_write_sent(&self) {
        self.camera_write_sent.store(true, Ordering::SeqCst);
    }

    pub fn camera_write_sent(&self) -> bool {
        self.camera_write_sent.load(Ordering::SeqCst)
    }

    pub fn enter_transaction(&self) -> TransactionGuard<'_> {
        self.transaction_in_flight.store(true, Ordering::SeqCst);
        TransactionGuard { latch: self }
    }

    pub fn enter_critical_region(&self) -> CriticalRegionGuard<'_> {
        self.critical_region_depth.fetch_add(1, Ordering::SeqCst);
        CriticalRegionGuard { latch: self }
    }

    /// True while an interrupt must be recorded rather than acted on.
    pub fn is_deferring(&self) -> bool {
        self.transaction_in_flight.load(Ordering::SeqCst) || self.critical_region_active()
    }

    pub fn critical_region_active(&self) -> bool {
        self.critical_region_depth.load(Ordering::SeqCst) > 0
    }

    /// Records one interrupt and returns how many were already pending.
    pub fn record_interrupt(&self) -> u8 {
        self.interrupts_seen
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |seen| {
                Some(seen.saturating_add(1))
            })
            .unwrap_or(u8::MAX)
    }

    /// Clears the pending interrupts and returns how many there were.
    pub fn take_interrupts(&self) -> u8 {
        self.interrupts_seen.swap(0, Ordering::SeqCst)
    }

    /// Applies pending interrupts to a completed transaction. Inside a critical
    /// region the interrupts stay pending for the region owner; otherwise a
    /// pending interrupt turns the completed result into an [`Interrupted`]
    /// error so the caller unwinds and the session closes normally.
    pub(crate) fn after_transaction<T>(&self, result: anyhow::Result<T>) -> anyhow::Result<T> {
        self.after_transaction_with(
            result,
            |_, interrupted| anyhow::Error::new(interrupted),
            anyhow::Error::context,
        )
    }

    /// Generalisation of [`Self::after_transaction`] for a result type whose
    /// error is not `anyhow::Error`. Inside a critical region, or with no
    /// interrupt pending, `result` is returned unchanged; otherwise
    /// `on_interrupted` builds a fresh failure from the successful value and
    /// the interrupt marker (so whatever the success recorded, such as a
    /// write journal, survives), and `with_interrupted_context` attaches the
    /// same marker to a result that had already failed on its own.
    pub(crate) fn after_transaction_with<T, E>(
        &self,
        result: Result<T, E>,
        on_interrupted: impl FnOnce(T, Interrupted) -> E,
        with_interrupted_context: impl FnOnce(E, Interrupted) -> E,
    ) -> Result<T, E> {
        if self.critical_region_active() || self.take_interrupts() == 0 {
            return result;
        }
        let interrupted = Interrupted {
            after_camera_write: self.camera_write_sent(),
        };
        match result {
            Ok(value) => Err(on_interrupted(value, interrupted)),
            Err(error) => Err(with_interrupted_context(error, interrupted)),
        }
    }
}

impl Default for InterruptLatch {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TransactionGuard<'_> {
    fn drop(&mut self) {
        self.latch
            .transaction_in_flight
            .store(false, Ordering::SeqCst);
    }
}

impl Drop for CriticalRegionGuard<'_> {
    fn drop(&mut self) {
        self.latch
            .critical_region_depth
            .fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::{InterruptLatch, Interrupted};

    #[test]
    fn interrupt_recorded_during_a_transaction_is_honoured_after_it_completes() {
        let latch = InterruptLatch::new();
        assert!(!latch.is_deferring());

        let guard = latch.enter_transaction();
        assert!(latch.is_deferring());
        assert_eq!(latch.record_interrupt(), 0);
        assert_eq!(latch.record_interrupt(), 1);
        drop(guard);
        assert!(!latch.is_deferring());

        let error = latch
            .after_transaction(Ok(42))
            .expect_err("a pending interrupt must replace the completed result");
        assert!(error.is::<Interrupted>());
        assert_eq!(latch.take_interrupts(), 0);

        let error = latch.after_transaction(Ok(1)).expect("nothing pending");
        assert_eq!(error, 1);
    }

    #[test]
    fn failed_transaction_keeps_its_error_and_gains_the_marker() {
        let latch = InterruptLatch::new();
        latch.record_interrupt();

        let error = latch
            .after_transaction::<()>(Err(anyhow::anyhow!("wire failure")))
            .expect_err("the failure must propagate");
        assert!(error.is::<Interrupted>());
        assert!(error.root_cause().to_string().contains("wire failure"));
    }

    #[test]
    fn interrupt_after_a_camera_write_is_reported_as_unknown_state() {
        let latch = InterruptLatch::new();
        latch.record_interrupt();
        let plain = latch
            .after_transaction(Ok(()))
            .expect_err("pending interrupt");
        let plain = plain
            .downcast_ref::<Interrupted>()
            .expect("the marker must be the error");
        assert!(!plain.after_camera_write);

        latch.mark_camera_write_sent();
        latch.record_interrupt();
        let error = latch
            .after_transaction::<()>(Err(anyhow::anyhow!("verification read failed")))
            .expect_err("pending interrupt");
        let marker = error
            .downcast_ref::<Interrupted>()
            .expect("the marker must be reachable through the context chain");
        assert!(marker.after_camera_write);
        assert!(marker.to_string().contains("DO NOT RETRY"));
        assert!(latch.camera_write_sent(), "the flag is sticky");
    }

    #[test]
    fn nested_critical_regions_end_only_when_the_outermost_drops() {
        let latch = InterruptLatch::new();
        let outer = latch.enter_critical_region();
        let inner = latch.enter_critical_region();
        drop(inner);
        assert!(
            latch.critical_region_active(),
            "dropping the inner region must not end the outer one"
        );
        drop(outer);
        assert!(!latch.critical_region_active());
    }

    #[test]
    fn critical_region_keeps_the_interrupt_pending_for_its_owner() {
        let latch = InterruptLatch::new();
        let region = latch.enter_critical_region();
        assert!(latch.is_deferring());
        latch.record_interrupt();

        let value = latch
            .after_transaction(Ok(7))
            .expect("the transport must not unwind a critical write");
        assert_eq!(value, 7);
        drop(region);
        assert!(!latch.is_deferring());
        assert_eq!(latch.take_interrupts(), 1);
    }
}
