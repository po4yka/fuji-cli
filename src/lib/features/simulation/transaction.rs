use std::fmt;

use anyhow::Context;
use binrw::{BinRead, BinWrite};

use crate::features::outcome::{OutcomeStatus, StateChangeAudit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationTransactionSuccess {
    AppliedAndVerified,
    NoChangeVerified,
}

impl SimulationTransactionSuccess {
    pub const fn audit(self) -> StateChangeAudit {
        match self {
            Self::AppliedAndVerified => {
                StateChangeAudit::ptp_accepted().with_semantic(OutcomeStatus::Succeeded)
            }
            Self::NoChangeVerified => {
                StateChangeAudit::not_attempted().with_semantic(OutcomeStatus::Succeeded)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationFailureState {
    RejectedWithoutChange,
    RollbackVerified,
    CameraStateUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationTransactionPhase {
    Preparation,
    ApplyWrite,
    ApplyReadback,
    RollbackWrite,
    RollbackReadback,
    /// The write itself finished (or was rolled back), but restoring the
    /// custom-setting slot selector the camera had before the write failed.
    SelectorRestore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulationWriteReceipt {
    pub setting: &'static str,
    pub property_code: u16,
}

#[derive(Debug)]
pub struct SimulationTransactionError {
    state: SimulationFailureState,
    phase: SimulationTransactionPhase,
    cause: anyhow::Error,
    rollback_error: Option<anyhow::Error>,
    rollback_readback_error: Option<anyhow::Error>,
    journal: Vec<SimulationWriteReceipt>,
    rollback_journal: Vec<SimulationWriteReceipt>,
    selector_restore_error: Option<anyhow::Error>,
}

impl SimulationTransactionError {
    pub(crate) fn preparation(healthy: bool, cause: anyhow::Error) -> Self {
        Self {
            state: if healthy {
                SimulationFailureState::RejectedWithoutChange
            } else {
                SimulationFailureState::CameraStateUnknown
            },
            phase: SimulationTransactionPhase::Preparation,
            cause,
            rollback_error: None,
            rollback_readback_error: None,
            journal: Vec::new(),
            rollback_journal: Vec::new(),
            selector_restore_error: None,
        }
    }

    pub fn state(&self) -> SimulationFailureState {
        self.state
    }

    pub fn phase(&self) -> SimulationTransactionPhase {
        self.phase
    }

    pub fn cause(&self) -> &anyhow::Error {
        &self.cause
    }

    pub fn rollback_error(&self) -> Option<&anyhow::Error> {
        self.rollback_error.as_ref()
    }

    pub fn rollback_readback_error(&self) -> Option<&anyhow::Error> {
        self.rollback_readback_error.as_ref()
    }

    pub fn journal(&self) -> &[SimulationWriteReceipt] {
        &self.journal
    }

    pub fn rollback_journal(&self) -> &[SimulationWriteReceipt] {
        &self.rollback_journal
    }

    /// The failure that left the camera on a slot other than the one it had
    /// before the write, when the selector could not be restored.
    pub fn selector_restore_error(&self) -> Option<&anyhow::Error> {
        self.selector_restore_error.as_ref()
    }

    /// The write succeeded, but the selector restore afterwards did not: the
    /// recipe is applied and verified, yet the camera is on an unknown slot.
    fn selector_restore_failed(cause: anyhow::Error) -> Self {
        Self {
            state: SimulationFailureState::CameraStateUnknown,
            phase: SimulationTransactionPhase::SelectorRestore,
            cause,
            rollback_error: None,
            rollback_readback_error: None,
            journal: Vec::new(),
            rollback_journal: Vec::new(),
            selector_restore_error: None,
        }
    }

    /// A failed write whose selector restore also failed: whatever the
    /// transaction outcome was, the camera's slot is now unknown too.
    fn with_selector_restore_failure(mut self, error: anyhow::Error) -> Self {
        self.state = SimulationFailureState::CameraStateUnknown;
        self.selector_restore_error = Some(error);
        self
    }
}

impl fmt::Display for SimulationTransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "simulation transaction failed during {:?}: {:?}: {}",
            self.phase, self.state, self.cause
        )?;
        if let Some(rollback_error) = &self.rollback_error {
            write!(formatter, "; rollback error: {rollback_error}")?;
        }
        if let Some(readback_error) = &self.rollback_readback_error {
            write!(formatter, "; rollback readback error: {readback_error}")?;
        }
        if let Some(restore_error) = &self.selector_restore_error {
            write!(
                formatter,
                "; the previously selected slot could not be restored: {restore_error}"
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for SimulationTransactionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SimulationPropertyChange {
    pub index: usize,
    pub setting: &'static str,
    pub property_code: u16,
    pub restorable: bool,
}

pub(crate) trait SimulationPropertyIo {
    fn is_healthy(&self) -> bool;

    fn get_prop<T>(&mut self, code: u16) -> anyhow::Result<T>
    where
        T: for<'a> BinRead<Args<'a> = ()>;

    fn set_prop<T>(&mut self, code: u16, value: &T) -> Result<(), SimulationPropertyWriteError>
    where
        T: for<'a> BinWrite<Args<'a> = ()>;

    fn firmware_option_write_value(
        &self,
        option: &str,
        logical_value: &str,
    ) -> anyhow::Result<i32> {
        anyhow::bail!(
            "simulation I/O has no firmware write capability for {option}={logical_value}"
        )
    }

    fn firmware_option_read_logical_value(
        &self,
        option: &str,
        wire_value: i32,
    ) -> anyhow::Result<&'static str> {
        anyhow::bail!(
            "simulation I/O has no firmware read capability for {option} wire value {wire_value}"
        )
    }
}

pub(crate) struct AuthorizedSimulationIo<'io> {
    authorized: crate::camera::AuthorizedPtp<'io>,
}

impl<'io> AuthorizedSimulationIo<'io> {
    pub(crate) fn new(authorized: crate::camera::AuthorizedPtp<'io>) -> Self {
        Self { authorized }
    }

    pub(crate) fn firmware_capability_profile(
        &self,
    ) -> anyhow::Result<&'static crate::generated::cameras::CameraFirmwareCapabilityProfile> {
        self.authorized.firmware_capability_profile()
    }

    pub(crate) fn is_healthy(&self) -> bool {
        self.authorized.is_healthy()
    }
}

impl SimulationPropertyIo for AuthorizedSimulationIo<'_> {
    fn is_healthy(&self) -> bool {
        self.authorized.is_healthy()
    }
    fn get_prop<T>(&mut self, code: u16) -> anyhow::Result<T>
    where
        T: for<'a> BinRead<Args<'a> = ()>,
    {
        self.authorized.get_prop(code)
    }
    fn set_prop<T>(&mut self, code: u16, value: &T) -> Result<(), SimulationPropertyWriteError>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        self.authorized
            .set_prop(code, value)
            .map_err(SimulationPropertyWriteError::unconfirmed)
    }
    fn firmware_option_write_value(
        &self,
        option: &str,
        logical_value: &str,
    ) -> anyhow::Result<i32> {
        self.authorized
            .firmware_option_write_value(option, logical_value)
    }
    fn firmware_option_read_logical_value(
        &self,
        option: &str,
        wire_value: i32,
    ) -> anyhow::Result<&'static str> {
        self.authorized
            .firmware_option_read_logical_value(option, wire_value)
    }
}

#[derive(Debug)]
pub(crate) struct SimulationPropertyWriteError {
    cause: anyhow::Error,
    property_confirmed: bool,
    binding_uncertain: bool,
}

impl SimulationPropertyWriteError {
    pub(crate) fn unconfirmed(cause: anyhow::Error) -> Self {
        Self {
            cause,
            property_confirmed: false,
            binding_uncertain: false,
        }
    }

    pub(crate) fn confirmed(cause: anyhow::Error) -> Self {
        Self {
            cause,
            property_confirmed: true,
            binding_uncertain: false,
        }
    }

    fn confirmed_with_uncertain_binding(cause: anyhow::Error) -> Self {
        Self {
            cause,
            property_confirmed: true,
            binding_uncertain: true,
        }
    }

    pub(crate) fn context(self, context: &'static str) -> Self {
        Self {
            cause: self.cause.context(context),
            ..self
        }
    }

    pub(crate) fn into_cause(self) -> anyhow::Error {
        self.cause
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporarySimulationSelectorState {
    RestoredAndVerified,
    Unknown,
}

#[derive(Debug)]
pub struct TemporarySimulationSelectorError {
    operation: Option<anyhow::Error>,
    restore: Option<anyhow::Error>,
    state: TemporarySimulationSelectorState,
}

impl TemporarySimulationSelectorError {
    pub fn operation_error(&self) -> Option<&anyhow::Error> {
        self.operation.as_ref()
    }

    pub fn restore_error(&self) -> Option<&anyhow::Error> {
        self.restore.as_ref()
    }

    pub fn state(&self) -> TemporarySimulationSelectorState {
        self.state
    }
}

impl fmt::Display for TemporarySimulationSelectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.operation, &self.restore, self.state) {
            (Some(operation), None, TemporarySimulationSelectorState::RestoredAndVerified) => {
                write!(
                    formatter,
                    "simulation slot read failed: {operation:#}; original slot selector was restored and verified"
                )
            }
            (Some(operation), Some(restore), TemporarySimulationSelectorState::Unknown) => write!(
                formatter,
                "simulation slot read failed: {operation:#}; restoring the original slot selector also failed: {restore:#}; selector state is unknown"
            ),
            (None, Some(restore), TemporarySimulationSelectorState::Unknown) => write!(
                formatter,
                "simulation slot read succeeded, but restoring the original slot selector failed: {restore:#}; selector state is unknown"
            ),
            (Some(operation), None, TemporarySimulationSelectorState::Unknown) => write!(
                formatter,
                "simulation slot read failed: {operation:#}; the PTP session is unhealthy and selector state is unknown"
            ),
            _ => formatter.write_str("temporary simulation selector transaction failed"),
        }
    }
}

impl std::error::Error for TemporarySimulationSelectorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.operation
            .as_ref()
            .or(self.restore.as_ref())
            .map(std::convert::AsRef::as_ref)
    }
}

trait SimulationSelectorIo {
    fn is_healthy(&self) -> bool;

    fn get_selector_raw(&mut self, property_code: u16) -> anyhow::Result<Vec<u8>>;

    fn set_selector_raw(&mut self, property_code: u16, value: &[u8]) -> anyhow::Result<()>;

    /// Validates a selector snapshot against the permit's descriptor form,
    /// without sending anything to the camera. Used to refuse an
    /// out-of-form snapshot before the first write of a restore sequence.
    fn validate_selector_raw(&self, property_code: u16, value: &[u8]) -> anyhow::Result<()>;
}

impl SimulationSelectorIo for AuthorizedSimulationIo<'_> {
    fn is_healthy(&self) -> bool {
        self.authorized.is_healthy()
    }

    fn get_selector_raw(&mut self, property_code: u16) -> anyhow::Result<Vec<u8>> {
        self.authorized.get_prop_raw(property_code)
    }

    fn set_selector_raw(&mut self, property_code: u16, value: &[u8]) -> anyhow::Result<()> {
        self.authorized
            .set_prop_raw(property_code, value)
            .map(|_| ())
    }

    fn validate_selector_raw(&self, property_code: u16, value: &[u8]) -> anyhow::Result<()> {
        self.authorized.validate_prop_raw(property_code, value)
    }
}

use crate::interrupt::{INTERRUPTS, InterruptLatch};

pub(crate) fn with_temporary_simulation_selector<T>(
    io: &mut AuthorizedSimulationIo<'_>,
    property_code: u16,
    operation: impl FnOnce(&mut AuthorizedSimulationIo<'_>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    with_temporary_simulation_selector_io(&INTERRUPTS, io, property_code, operation)
}

/// Runs `operation` with the slot selector temporarily switched, then
/// restores and verifies the original selector. The whole scope is one
/// interrupt critical region: a Ctrl-C anywhere inside it, including between
/// PTP transactions, is recorded rather than acted on, so the restore always
/// runs. The recorded interrupt is honoured once the selector is back.
fn with_temporary_simulation_selector_io<IO, T>(
    interrupts: &InterruptLatch,
    io: &mut IO,
    property_code: u16,
    operation: impl FnOnce(&mut IO) -> anyhow::Result<T>,
) -> anyhow::Result<T>
where
    IO: SimulationSelectorIo,
{
    let region = interrupts.enter_critical_region();
    let result = restore_selector_after(io, property_code, operation);
    drop(region);
    interrupts.after_transaction(result)
}

/// Runs a simulation write with the camera's slot selector restored
/// afterwards, so `simulation set` and `simulation import` leave the camera on
/// the slot it had before, exactly like the read paths. The whole scope is an
/// interrupt critical region. A failed restore after a successful write, or
/// after a failed one, is reported as an unknown camera state carrying the
/// restore error; an unhealthy session skips the restore entirely.
pub(crate) fn with_restored_simulation_selector<T>(
    io: &mut AuthorizedSimulationIo<'_>,
    property_code: u16,
    operation: impl FnOnce(&mut AuthorizedSimulationIo<'_>) -> Result<T, SimulationTransactionError>,
) -> Result<T, SimulationTransactionError> {
    with_restored_simulation_selector_io(&INTERRUPTS, io, property_code, operation)
}

fn with_restored_simulation_selector_io<IO, T>(
    interrupts: &InterruptLatch,
    io: &mut IO,
    property_code: u16,
    operation: impl FnOnce(&mut IO) -> Result<T, SimulationTransactionError>,
) -> Result<T, SimulationTransactionError>
where
    IO: SimulationSelectorIo,
{
    let region = interrupts.enter_critical_region();
    let original = io
        .get_selector_raw(property_code)
        .context("snapshotting the original simulation slot selector")
        .map_err(|cause| SimulationTransactionError::preparation(io.is_healthy(), cause))?;
    io.validate_selector_raw(property_code, &original)
        .with_context(|| {
            format!(
                "simulation slot selector (PTP property 0x{property_code:04x}): the camera's current slot value is outside the verified descriptor form, so nothing was written"
            )
        })
        .map_err(|cause| SimulationTransactionError::preparation(io.is_healthy(), cause))?;
    let result = operation(io);
    let restore = if io.is_healthy() {
        restore_selector(io, property_code, &original)
    } else {
        Err(anyhow::anyhow!(
            "PTP session became unhealthy during the simulation write; the slot selector was not restored"
        ))
    };
    drop(region);
    match (result, restore) {
        (result, Ok(())) => result,
        (Ok(_), Err(restore)) => Err(SimulationTransactionError::selector_restore_failed(restore)),
        (Err(error), Err(restore)) => Err(error.with_selector_restore_failure(restore)),
    }
}

fn restore_selector_after<IO, T>(
    io: &mut IO,
    property_code: u16,
    operation: impl FnOnce(&mut IO) -> anyhow::Result<T>,
) -> anyhow::Result<T>
where
    IO: SimulationSelectorIo,
{
    let original = io
        .get_selector_raw(property_code)
        .context("snapshotting the original simulation slot selector")?;
    io.validate_selector_raw(property_code, &original)
        .with_context(|| {
            format!(
                "simulation slot selector (PTP property 0x{property_code:04x}): the camera's current slot value is outside the verified descriptor form"
            )
        })?;
    let operation = operation(io);

    if !io.is_healthy() {
        let operation = operation.err().unwrap_or_else(|| {
            anyhow::anyhow!("PTP session became unhealthy during simulation slot read")
        });
        return Err(TemporarySimulationSelectorError {
            operation: Some(operation),
            restore: None,
            state: TemporarySimulationSelectorState::Unknown,
        }
        .into());
    }

    let restore = restore_selector(io, property_code, &original);
    match (operation, restore) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(operation), Ok(())) => Err(TemporarySimulationSelectorError {
            operation: Some(operation),
            restore: None,
            state: TemporarySimulationSelectorState::RestoredAndVerified,
        }
        .into()),
        (Ok(_), Err(restore)) => Err(TemporarySimulationSelectorError {
            operation: None,
            restore: Some(restore),
            state: TemporarySimulationSelectorState::Unknown,
        }
        .into()),
        (Err(operation), Err(restore)) => Err(TemporarySimulationSelectorError {
            operation: Some(operation),
            restore: Some(restore),
            state: TemporarySimulationSelectorState::Unknown,
        }
        .into()),
    }
}

fn restore_selector<IO: SimulationSelectorIo>(
    io: &mut IO,
    property_code: u16,
    original: &[u8],
) -> anyhow::Result<()> {
    io.set_selector_raw(property_code, original)?;
    let readback = io.get_selector_raw(property_code)?;
    anyhow::ensure!(
        readback == original,
        "restored simulation selector readback does not match the original raw value"
    );
    Ok(())
}

pub(crate) struct SelectedSimulationIo<'io, IO, Selector> {
    io: &'io mut IO,
    selector: Selector,
    /// Set once the target slot has been selected and verified for this
    /// adapter scope. Every property access after that point trusts the
    /// selection instead of rewriting it, and only re-verifies it. Cleared
    /// again the moment `verify_selector` observes drift, so the very next
    /// access reselects instead of trusting a selection already known to be
    /// stale.
    selected: bool,
}

impl<'io, IO, Selector> SelectedSimulationIo<'io, IO, Selector> {
    pub(crate) fn new(io: &'io mut IO, selector: Selector) -> Self {
        Self {
            io,
            selector,
            selected: false,
        }
    }
}

impl<IO, Selector> SimulationPropertyIo for SelectedSimulationIo<'_, IO, Selector>
where
    IO: SimulationPropertyIo,
    Selector: crate::ptp::option::SimulationSetting + PartialEq,
{
    fn is_healthy(&self) -> bool {
        self.io.is_healthy()
    }

    fn get_prop<T>(&mut self, code: u16) -> anyhow::Result<T>
    where
        T: for<'a> BinRead<Args<'a> = ()>,
    {
        self.ensure_selected()?;
        let value = self.io.get_prop(code)?;
        self.verify_selector()?;
        Ok(value)
    }

    fn set_prop<T>(&mut self, code: u16, value: &T) -> Result<(), SimulationPropertyWriteError>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        self.ensure_selected()
            .map_err(SimulationPropertyWriteError::unconfirmed)?;
        self.io.set_prop(code, value)?;
        self.verify_selector()
            .map_err(SimulationPropertyWriteError::confirmed_with_uncertain_binding)
    }

    fn firmware_option_write_value(
        &self,
        option: &str,
        logical_value: &str,
    ) -> anyhow::Result<i32> {
        self.io.firmware_option_write_value(option, logical_value)
    }

    fn firmware_option_read_logical_value(
        &self,
        option: &str,
        wire_value: i32,
    ) -> anyhow::Result<&'static str> {
        self.io
            .firmware_option_read_logical_value(option, wire_value)
    }
}

impl<IO, Selector> SelectedSimulationIo<'_, IO, Selector>
where
    IO: SimulationPropertyIo,
    Selector: crate::ptp::option::SimulationSetting + PartialEq,
{
    /// Selects and verifies the target slot exactly once for this adapter's
    /// scope, on its first property access. Later accesses in the same scope
    /// trust that selection and skip straight to `verify_selector`; the
    /// selector write is a persistent camera state change, so repeating it
    /// on every access would multiply PTP transactions for no benefit once
    /// the slot is confirmed selected.
    fn ensure_selected(&mut self) -> anyhow::Result<()> {
        if self.selected {
            return Ok(());
        }
        Selector::try_push_to(&self.selector, self.io)
            .map_err(SimulationPropertyWriteError::into_cause)?;
        self.verify_selector()?;
        self.selected = true;
        Ok(())
    }

    /// Re-reads the active slot and compares it against the target. Any
    /// failure here (a transport error or an observed mismatch) clears
    /// `selected` so the next access forces a fresh select-and-verify
    /// instead of trusting a selection that is now known to be stale; this
    /// matters most for the rollback path, which must never write to the
    /// wrong slot after drift has already been detected once.
    fn verify_selector(&mut self) -> anyhow::Result<()> {
        let selected = match Selector::try_pull_from(self.io) {
            Ok(selected) => selected,
            Err(err) => {
                self.selected = false;
                return Err(err);
            }
        };
        if selected != self.selector {
            self.selected = false;
            anyhow::bail!("selected simulation slot changed during profile property access");
        }
        Ok(())
    }
}

pub(crate) trait SimulationTransactionProfile: Clone + PartialEq {
    fn changes_from(&self, original: &Self) -> anyhow::Result<Vec<SimulationPropertyChange>>;

    fn pull_from<IO: SimulationPropertyIo>(io: &mut IO) -> anyhow::Result<Self>;

    fn push_change<IO: SimulationPropertyIo>(
        &self,
        change: SimulationPropertyChange,
        io: &mut IO,
    ) -> Result<(), SimulationPropertyWriteError>;

    fn verify_change<IO: SimulationPropertyIo>(
        &self,
        change: SimulationPropertyChange,
        io: &mut IO,
    ) -> Result<(), SimulationPropertyWriteError>;
}

pub(crate) fn execute_simulation_transaction<P, IO>(
    io: &mut IO,
    original: &P,
    candidate: &P,
) -> Result<SimulationTransactionSuccess, SimulationTransactionError>
where
    P: SimulationTransactionProfile,
    IO: SimulationPropertyIo,
{
    let changes = candidate
        .changes_from(original)
        .map_err(|cause| SimulationTransactionError::preparation(io.is_healthy(), cause))?;
    if changes.is_empty() {
        return match P::pull_from(io) {
            Ok(readback) if readback == *candidate => {
                Ok(SimulationTransactionSuccess::NoChangeVerified)
            }
            Ok(readback) if readback == *original => Err(SimulationTransactionError {
                state: SimulationFailureState::RejectedWithoutChange,
                phase: SimulationTransactionPhase::ApplyReadback,
                cause: anyhow::anyhow!(
                    "simulation candidate contains no writable changes and was not observed"
                ),
                rollback_error: None,
                rollback_readback_error: None,
                journal: Vec::new(),
                rollback_journal: Vec::new(),
                selector_restore_error: None,
            }),
            Ok(_) => Err(SimulationTransactionError {
                state: SimulationFailureState::CameraStateUnknown,
                phase: SimulationTransactionPhase::ApplyReadback,
                cause: anyhow::anyhow!("simulation readback did not match the original profile"),
                rollback_error: None,
                rollback_readback_error: None,
                journal: Vec::new(),
                rollback_journal: Vec::new(),
                selector_restore_error: None,
            }),
            Err(cause) => Err(SimulationTransactionError {
                state: SimulationFailureState::CameraStateUnknown,
                phase: SimulationTransactionPhase::ApplyReadback,
                cause,
                rollback_error: None,
                rollback_readback_error: None,
                journal: Vec::new(),
                rollback_journal: Vec::new(),
                selector_restore_error: None,
            }),
        };
    }

    let mut journal = Vec::new();
    for change in changes {
        if let Err(error) = candidate.push_change(change, io) {
            // `property_confirmed` means the framed PTP response accepted the
            // write and only the readback failed (the generated
            // `push_change` reports that through `confirmed`), or the
            // selector drifted after a confirmed write. A transport-ambiguous
            // write is reported unconfirmed and poisons the session instead;
            // `recover_original` refuses any rollback on a poisoned session.
            if error.property_confirmed {
                journal.push(change);
            }
            return Err(recover_original(
                io,
                original,
                SimulationTransactionPhase::ApplyWrite,
                error.cause,
                &journal,
                error.binding_uncertain,
            ));
        }
        journal.push(change);
        if let Err(error) = candidate.verify_change(change, io) {
            return Err(recover_original(
                io,
                original,
                SimulationTransactionPhase::ApplyReadback,
                error.cause,
                &journal,
                error.binding_uncertain,
            ));
        }
    }

    match P::pull_from(io) {
        Ok(readback) if readback == *candidate => {
            Ok(SimulationTransactionSuccess::AppliedAndVerified)
        }
        Ok(_) => Err(recover_original(
            io,
            original,
            SimulationTransactionPhase::ApplyReadback,
            anyhow::anyhow!("simulation readback did not match the requested profile"),
            &journal,
            false,
        )),
        Err(cause) => Err(recover_original(
            io,
            original,
            SimulationTransactionPhase::ApplyReadback,
            cause,
            &journal,
            false,
        )),
    }
}

fn recover_original<P, IO>(
    io: &mut IO,
    original: &P,
    primary_phase: SimulationTransactionPhase,
    cause: anyhow::Error,
    journal: &[SimulationPropertyChange],
    mut binding_uncertain: bool,
) -> SimulationTransactionError
where
    P: SimulationTransactionProfile,
    IO: SimulationPropertyIo,
{
    let receipts = journal.iter().copied().map(Into::into).collect();
    if !io.is_healthy() {
        return SimulationTransactionError {
            state: SimulationFailureState::CameraStateUnknown,
            phase: primary_phase,
            cause,
            rollback_error: None,
            rollback_readback_error: None,
            journal: receipts,
            rollback_journal: Vec::new(),
            selector_restore_error: None,
        };
    }

    if journal.is_empty() {
        return match P::pull_from(io) {
            Ok(readback) if readback == *original => SimulationTransactionError {
                state: SimulationFailureState::RejectedWithoutChange,
                phase: primary_phase,
                cause,
                rollback_error: None,
                rollback_readback_error: None,
                journal: receipts,
                rollback_journal: Vec::new(),
                selector_restore_error: None,
            },
            Ok(_) => SimulationTransactionError {
                state: SimulationFailureState::CameraStateUnknown,
                phase: SimulationTransactionPhase::RollbackReadback,
                cause,
                rollback_error: None,
                rollback_readback_error: Some(anyhow::anyhow!(
                    "simulation recovery readback did not match the original profile"
                )),
                journal: receipts,
                rollback_journal: Vec::new(),
                selector_restore_error: None,
            },
            Err(error) => SimulationTransactionError {
                state: SimulationFailureState::CameraStateUnknown,
                phase: SimulationTransactionPhase::RollbackReadback,
                cause,
                rollback_error: None,
                rollback_readback_error: Some(error),
                journal: receipts,
                rollback_journal: Vec::new(),
                selector_restore_error: None,
            },
        };
    }

    let mut rollback_journal = Vec::new();
    let mut rollback_error = None;
    let mut rollback_readback_error = None;
    let mut all_restorable = true;
    for change in journal.iter().rev().copied() {
        if !change.restorable {
            all_restorable = false;
            continue;
        }
        match original.push_change(change, io) {
            Ok(()) => {
                rollback_journal.push(change.into());
                if let Err(error) = original.verify_change(change, io) {
                    binding_uncertain |= error.binding_uncertain;
                    rollback_readback_error.get_or_insert_with(|| {
                        error.cause.context(
                            "simulation recovery readback did not match the original profile",
                        )
                    });
                    if !io.is_healthy() {
                        break;
                    }
                }
            }
            Err(error) => {
                if error.property_confirmed {
                    rollback_journal.push(change.into());
                }
                binding_uncertain |= error.binding_uncertain;
                rollback_error.get_or_insert(error.cause);
                if !io.is_healthy() {
                    break;
                }
            }
        }
    }

    if !io.is_healthy() {
        return SimulationTransactionError {
            state: SimulationFailureState::CameraStateUnknown,
            phase: if rollback_readback_error.is_some() {
                SimulationTransactionPhase::RollbackReadback
            } else {
                SimulationTransactionPhase::RollbackWrite
            },
            cause,
            rollback_error,
            rollback_readback_error,
            journal: receipts,
            rollback_journal,
            selector_restore_error: None,
        };
    }

    match P::pull_from(io) {
        Ok(readback)
            if readback == *original
                && all_restorable
                && !binding_uncertain
                && rollback_readback_error.is_none() =>
        {
            SimulationTransactionError {
                state: SimulationFailureState::RollbackVerified,
                phase: primary_phase,
                cause,
                rollback_error,
                rollback_readback_error,
                journal: receipts,
                rollback_journal,
                selector_restore_error: None,
            }
        }
        Ok(_) => {
            rollback_readback_error.get_or_insert_with(|| {
                anyhow::anyhow!("simulation recovery readback did not match the original profile")
            });
            SimulationTransactionError {
                state: SimulationFailureState::CameraStateUnknown,
                phase: SimulationTransactionPhase::RollbackReadback,
                cause,
                rollback_error,
                rollback_readback_error,
                journal: receipts,
                rollback_journal,
                selector_restore_error: None,
            }
        }
        Err(error) => SimulationTransactionError {
            state: SimulationFailureState::CameraStateUnknown,
            phase: SimulationTransactionPhase::RollbackReadback,
            cause,
            rollback_error,
            rollback_readback_error: rollback_readback_error.or(Some(error)),
            journal: receipts,
            rollback_journal,
            selector_restore_error: None,
        },
    }
}

impl From<SimulationPropertyChange> for SimulationWriteReceipt {
    fn from(change: SimulationPropertyChange) -> Self {
        Self {
            setting: change.setting,
            property_code: change.property_code,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use binrw::{BinRead, BinWrite};

    use crate::ptp::codec;

    use super::{
        SelectedSimulationIo, SimulationFailureState, SimulationPropertyChange,
        SimulationPropertyIo, SimulationPropertyWriteError, SimulationSelectorIo,
        SimulationTransactionError, SimulationTransactionPhase, SimulationTransactionProfile,
        SimulationTransactionSuccess, TemporarySimulationSelectorError,
        TemporarySimulationSelectorState, execute_simulation_transaction,
        with_restored_simulation_selector_io, with_temporary_simulation_selector_io,
    };

    #[derive(Clone, Copy, PartialEq, BinRead, BinWrite)]
    #[brw(little)]
    struct TestSelector(u16);

    impl crate::ptp::option::SimulationSetting for TestSelector {
        fn prop_code() -> u16 {
            0xd000
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct TestProfile {
        first: Option<u16>,
        second: Option<u16>,
        third: Option<u16>,
    }

    impl SimulationTransactionProfile for TestProfile {
        fn changes_from(&self, original: &Self) -> anyhow::Result<Vec<SimulationPropertyChange>> {
            let mut changes = Vec::new();
            if self.first != original.first && self.first.is_some() {
                changes.push(SimulationPropertyChange {
                    index: 0,
                    setting: "first",
                    property_code: 0xd001,
                    restorable: original.first.is_some(),
                });
            }
            if self.second != original.second && self.second.is_some() {
                changes.push(SimulationPropertyChange {
                    index: 1,
                    setting: "second",
                    property_code: 0xd002,
                    restorable: original.second.is_some(),
                });
            }
            if self.third != original.third && self.third.is_some() {
                changes.push(SimulationPropertyChange {
                    index: 2,
                    setting: "third",
                    property_code: 0xd003,
                    restorable: original.third.is_some(),
                });
            }
            Ok(changes)
        }

        fn pull_from<IO: SimulationPropertyIo>(io: &mut IO) -> anyhow::Result<Self> {
            Ok(Self {
                first: Some(io.get_prop(0xd001)?),
                second: Some(io.get_prop(0xd002)?),
                third: Some(io.get_prop(0xd003)?),
            })
        }

        fn push_change<IO: SimulationPropertyIo>(
            &self,
            change: SimulationPropertyChange,
            io: &mut IO,
        ) -> Result<(), SimulationPropertyWriteError> {
            match change.index {
                0 => io.set_prop(0xd001, &self.first.expect("planned value")),
                1 => io.set_prop(0xd002, &self.second.expect("planned value")),
                2 => io.set_prop(0xd003, &self.third.expect("planned value")),
                _ => Err(SimulationPropertyWriteError::unconfirmed(anyhow::anyhow!(
                    "unknown test change index"
                ))),
            }
        }

        fn verify_change<IO: SimulationPropertyIo>(
            &self,
            change: SimulationPropertyChange,
            io: &mut IO,
        ) -> Result<(), SimulationPropertyWriteError> {
            let (expected, observed) = match change.index {
                0 => (
                    self.first.expect("planned value"),
                    io.get_prop::<u16>(0xd001),
                ),
                1 => (
                    self.second.expect("planned value"),
                    io.get_prop::<u16>(0xd002),
                ),
                2 => (
                    self.third.expect("planned value"),
                    io.get_prop::<u16>(0xd003),
                ),
                _ => {
                    return Err(SimulationPropertyWriteError::confirmed(anyhow::anyhow!(
                        "unknown test change index"
                    )));
                }
            };
            let observed = observed.map_err(SimulationPropertyWriteError::confirmed)?;
            if observed == expected {
                Ok(())
            } else {
                Err(SimulationPropertyWriteError::confirmed(anyhow::anyhow!(
                    "test setting readback mismatch"
                )))
            }
        }
    }

    #[derive(Default)]
    struct FakeSimulationPropertyIo {
        healthy: bool,
        properties: BTreeMap<u16, Vec<u8>>,
        writes: Vec<u16>,
        faults: VecDeque<Fault>,
        reads: Vec<u16>,
        /// When set to a property code, `validate_selector_raw` rejects a
        /// snapshot of that property, simulating a camera sitting on a slot
        /// value outside the permit's verified descriptor form.
        reject_selector_validation: Option<u16>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fault {
        IgnoreWrite(u16),
        RejectWrite(u16),
        PoisonWrite(u16),
        PoisonRead(u16),
        ReturnRead(u16, u16),
        SwitchSelectorBeforeWrite(u16, u16),
    }

    impl FakeSimulationPropertyIo {
        fn with_profile(profile: &TestProfile) -> Self {
            let mut properties = BTreeMap::new();
            properties.insert(0xd001, codec::encode(&profile.first.unwrap()).unwrap());
            properties.insert(0xd002, codec::encode(&profile.second.unwrap()).unwrap());
            properties.insert(0xd003, codec::encode(&profile.third.unwrap()).unwrap());
            Self {
                healthy: true,
                properties,
                writes: Vec::new(),
                faults: VecDeque::new(),
                reads: Vec::new(),
                reject_selector_validation: None,
            }
        }

        fn ignore_write(mut self, property_code: u16) -> Self {
            self.faults.push_back(Fault::IgnoreWrite(property_code));
            self
        }

        fn reject_write(mut self, property_code: u16) -> Self {
            self.faults.push_back(Fault::RejectWrite(property_code));
            self
        }

        fn poison_write(mut self, property_code: u16) -> Self {
            self.faults.push_back(Fault::PoisonWrite(property_code));
            self
        }

        fn poison_read(mut self, property_code: u16) -> Self {
            self.faults.push_back(Fault::PoisonRead(property_code));
            self
        }

        fn return_read(mut self, property_code: u16, value: u16) -> Self {
            self.faults
                .push_back(Fault::ReturnRead(property_code, value));
            self
        }

        fn switch_selector_before_write(mut self, property_code: u16, selector: u16) -> Self {
            self.faults
                .push_back(Fault::SwitchSelectorBeforeWrite(property_code, selector));
            self
        }

        fn reject_selector_validation(mut self, property_code: u16) -> Self {
            self.reject_selector_validation = Some(property_code);
            self
        }
    }

    impl SimulationPropertyIo for FakeSimulationPropertyIo {
        fn is_healthy(&self) -> bool {
            self.healthy
        }

        fn get_prop<T>(&mut self, code: u16) -> anyhow::Result<T>
        where
            T: for<'a> BinRead<Args<'a> = ()>,
        {
            self.reads.push(code);
            if self.faults.front() == Some(&Fault::PoisonRead(code)) {
                self.faults.pop_front();
                self.healthy = false;
                anyhow::bail!("poisoned read for 0x{code:04x}");
            }
            if let Some(Fault::ReturnRead(property_code, value)) = self.faults.front().copied()
                && property_code == code
            {
                self.faults.pop_front();
                return Ok(codec::decode_exact(&codec::encode(&value)?)?);
            }
            Ok(codec::decode_exact(
                self.properties.get(&code).expect("test property"),
            )?)
        }

        fn set_prop<T>(&mut self, code: u16, value: &T) -> Result<(), SimulationPropertyWriteError>
        where
            T: for<'a> BinWrite<Args<'a> = ()>,
        {
            if let Some(Fault::SwitchSelectorBeforeWrite(property_code, selector)) =
                self.faults.front().copied()
                && property_code == code
            {
                self.faults.pop_front();
                self.properties
                    .insert(0xd000, codec::encode(&selector).expect("test selector"));
            }
            if self.faults.front() == Some(&Fault::RejectWrite(code)) {
                self.faults.pop_front();
                return Err(SimulationPropertyWriteError::unconfirmed(anyhow::anyhow!(
                    "framed PTP rejection for 0x{code:04x}"
                )));
            }
            if self.faults.front() == Some(&Fault::PoisonWrite(code)) {
                self.faults.pop_front();
                self.healthy = false;
                return Err(SimulationPropertyWriteError::unconfirmed(anyhow::anyhow!(
                    "poisoned write for 0x{code:04x}"
                )));
            }
            self.writes.push(code);
            if self.faults.front() == Some(&Fault::IgnoreWrite(code)) {
                self.faults.pop_front();
                return Ok(());
            }
            let encoded = codec::encode(value).map_err(|error| {
                SimulationPropertyWriteError::unconfirmed(anyhow::Error::from(error))
            })?;
            self.properties.insert(code, encoded);
            Ok(())
        }
    }

    impl SimulationSelectorIo for FakeSimulationPropertyIo {
        fn is_healthy(&self) -> bool {
            self.healthy
        }

        fn get_selector_raw(&mut self, property_code: u16) -> anyhow::Result<Vec<u8>> {
            self.reads.push(property_code);
            if self.faults.front() == Some(&Fault::PoisonRead(property_code)) {
                self.faults.pop_front();
                self.healthy = false;
                anyhow::bail!("poisoned selector read for 0x{property_code:04x}");
            }
            if let Some(Fault::ReturnRead(code, value)) = self.faults.front().copied()
                && code == property_code
            {
                self.faults.pop_front();
                return Ok(codec::encode(&value)?);
            }
            self.properties
                .get(&property_code)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing selector property 0x{property_code:04x}"))
        }

        fn set_selector_raw(&mut self, property_code: u16, value: &[u8]) -> anyhow::Result<()> {
            if self.faults.front() == Some(&Fault::RejectWrite(property_code)) {
                self.faults.pop_front();
                anyhow::bail!("framed selector rejection for 0x{property_code:04x}");
            }
            if self.faults.front() == Some(&Fault::PoisonWrite(property_code)) {
                self.faults.pop_front();
                self.healthy = false;
                anyhow::bail!("poisoned selector write for 0x{property_code:04x}");
            }
            self.writes.push(property_code);
            self.properties.insert(property_code, value.to_vec());
            Ok(())
        }

        fn validate_selector_raw(&self, property_code: u16, _value: &[u8]) -> anyhow::Result<()> {
            if self.reject_selector_validation == Some(property_code) {
                anyhow::bail!(
                    "test simulation slot selector 0x{property_code:04x} snapshot is outside the fake's verified descriptor form"
                );
            }
            Ok(())
        }
    }

    #[test]
    fn transaction_writes_only_changed_properties_and_verifies_readback() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(1),
            second: Some(3),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original);

        let result = execute_simulation_transaction(&mut io, &original, &candidate);

        assert_eq!(
            result.unwrap(),
            SimulationTransactionSuccess::AppliedAndVerified
        );
        assert_eq!(io.writes, [0xd002]);
    }

    #[test]
    fn accepted_setting_write_is_read_back_before_the_next_write() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original).ignore_write(0xd001);

        execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("PTP OK without the requested state change must fail semantically");

        assert!(
            !io.writes.contains(&0xd002),
            "the next setting must not be written before the first setting is verified"
        );
    }

    #[test]
    fn healthy_rejection_rolls_back_confirmed_writes_in_reverse_order() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(6),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original).reject_write(0xd003);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("the second property must be rejected");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::RollbackVerified
        );
        assert_eq!(io.writes, [0xd001, 0xd002, 0xd002, 0xd001]);
        assert_eq!(error.journal().len(), 2);
        assert_eq!(error.rollback_journal().len(), 2);
    }

    #[test]
    fn poisoned_apply_stops_without_rollback_or_readback() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original).poison_write(0xd002);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("the second write must poison the session");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::CameraStateUnknown
        );
        assert_eq!(io.writes, [0xd001]);
        assert_eq!(io.reads, [0xd001]);
        assert_eq!(error.journal().len(), 1);
        assert!(error.rollback_journal().is_empty());
    }

    #[test]
    fn poisoned_apply_readback_stops_without_rollback() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original).poison_read(0xd001);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("apply readback must poison the session");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::CameraStateUnknown
        );
        assert_eq!(io.writes, [0xd001]);
        assert_eq!(io.reads, [0xd001]);
        assert!(error.rollback_journal().is_empty());
    }

    #[test]
    fn apply_readback_mismatch_rolls_back_and_verifies_original() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original).return_read(0xd001, 99);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("a mismatching apply readback must trigger recovery");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::RollbackVerified
        );
        assert_eq!(io.writes, [0xd001, 0xd001]);
        assert_eq!(error.rollback_journal().len(), 1);
    }

    #[test]
    fn failed_rollback_with_mismatching_readback_is_unknown() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(6),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original)
            .reject_write(0xd003)
            .reject_write(0xd002);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("apply and rollback writes must be rejected");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::CameraStateUnknown
        );
        assert_eq!(
            error.phase(),
            super::SimulationTransactionPhase::RollbackReadback
        );
        assert_eq!(io.writes, [0xd001, 0xd002, 0xd001]);
        assert!(error.rollback_error().is_some());
    }

    #[test]
    fn accepted_rollback_write_without_original_value_is_not_verified() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(6),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original)
            .reject_write(0xd003)
            .ignore_write(0xd002);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("PTP OK without restoring the original value must fail semantically");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::CameraStateUnknown
        );
        assert_eq!(
            error.phase(),
            super::SimulationTransactionPhase::RollbackReadback
        );
        assert!(
            error.rollback_readback_error().is_some_and(|error| error
                .to_string()
                .contains("did not match the original profile")),
            "the typed outcome must retain the rollback semantic mismatch"
        );
    }

    #[test]
    fn poisoned_rollback_write_stops_recovery_without_readback() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(6),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original)
            .reject_write(0xd003)
            .poison_write(0xd002);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("rollback of the second property must poison the session");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::CameraStateUnknown
        );
        assert_eq!(
            error.phase(),
            super::SimulationTransactionPhase::RollbackWrite
        );
        assert_eq!(io.writes, [0xd001, 0xd002]);
        assert_eq!(io.reads, [0xd001, 0xd002]);
        assert!(error.rollback_journal().is_empty());
    }

    #[test]
    fn poisoned_rollback_readback_is_unknown_after_reverse_recovery() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(6),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original)
            .reject_write(0xd003)
            .poison_read(0xd001);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("rollback readback must poison the session");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::CameraStateUnknown
        );
        assert_eq!(
            error.phase(),
            super::SimulationTransactionPhase::RollbackReadback
        );
        assert_eq!(io.writes, [0xd001, 0xd002, 0xd002, 0xd001]);
        assert_eq!(io.reads, [0xd001, 0xd002, 0xd002, 0xd001]);
        assert_eq!(error.rollback_journal().len(), 2);
        assert!(error.rollback_readback_error().is_some());
    }

    #[test]
    fn rollback_write_and_readback_errors_are_both_preserved() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(6),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original)
            .reject_write(0xd003)
            .reject_write(0xd002)
            .poison_read(0xd001);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("rollback write and readback must both fail");

        assert!(error.rollback_error().is_some());
        assert!(error.rollback_readback_error().is_some());
        let diagnostic = error.to_string();
        assert!(diagnostic.contains("rollback error"));
        assert!(diagnostic.contains("rollback readback error"));
    }

    #[test]
    fn non_restorable_confirmed_write_never_reports_verified_rollback() {
        let original = TestProfile {
            first: None,
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(4),
            third: Some(6),
        };
        let wire_before = TestProfile {
            first: Some(9),
            second: Some(2),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&wire_before).reject_write(0xd003);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("the third write must fail after a non-restorable write");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::CameraStateUnknown
        );
        assert_eq!(io.writes, [0xd001, 0xd002, 0xd002]);
        assert_eq!(error.journal().len(), 2);
        assert_eq!(error.rollback_journal().len(), 1);
    }

    #[test]
    fn no_change_is_verified_without_writes() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&profile);

        let result = execute_simulation_transaction(&mut io, &profile, &profile).unwrap();

        assert_eq!(result, SimulationTransactionSuccess::NoChangeVerified);
        assert!(io.writes.is_empty());
        assert_eq!(io.reads, [0xd001, 0xd002, 0xd003]);
    }

    #[test]
    fn rejection_before_any_confirmed_write_is_verified_unchanged() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(2),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&original).reject_write(0xd001);

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("the only write must be rejected");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::RejectedWithoutChange
        );
        assert!(error.journal().is_empty());
        assert!(io.writes.is_empty());
        assert_eq!(io.reads, [0xd001, 0xd002, 0xd003]);
    }

    #[test]
    fn selected_io_selects_the_target_once_per_scope_and_verifies_every_access() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&profile);

        {
            let mut selected = SelectedSimulationIo::new(&mut io, TestSelector(7));
            selected.set_prop(0xd001, &3_u16).unwrap();
            let _: u16 = selected.get_prop(0xd002).unwrap();
            let _: u16 = selected.get_prop(0xd003).unwrap();
        }

        assert_eq!(
            io.writes
                .iter()
                .filter(|property| **property == 0xd000)
                .count(),
            1,
            "reading and writing a multi-property profile through one adapter \
             scope must select the target slot exactly once"
        );
        assert_eq!(io.writes, [0xd000, 0xd001]);
        assert_eq!(
            io.reads,
            [0xd000, 0xd000, 0xd002, 0xd000, 0xd003, 0xd000],
            "every access is still verified against the selector after the fact"
        );
    }

    #[test]
    fn interrupt_inside_the_temporary_selector_scope_restores_before_it_is_honoured() {
        use crate::interrupt::{InterruptLatch, Interrupted};

        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let latch = InterruptLatch::new();
        let mut io = FakeSimulationPropertyIo::with_profile(&profile);
        let original = codec::encode(&9_u16).expect("test selector");
        io.properties.insert(0xd000, original.clone());

        let error = with_temporary_simulation_selector_io::<_, ()>(&latch, &mut io, 0xd000, |io| {
            assert!(
                latch.is_deferring(),
                "the whole scope must defer interrupts, not only each transaction"
            );
            let mut selected = SelectedSimulationIo::new(io, TestSelector(3));
            let _: u16 = selected.get_prop(0xd001)?;
            // A Ctrl-C lands between two PTP transactions of the scope.
            latch.record_interrupt();
            let mut selected = SelectedSimulationIo::new(io, TestSelector(4));
            let _: u16 = selected.get_prop(0xd001)?;
            Ok(())
        })
        .expect_err("the recorded interrupt must be honoured after the scope");

        assert!(!latch.is_deferring());
        assert_eq!(
            latch.take_interrupts(),
            0,
            "the scope drained the interrupt"
        );
        let marker = error
            .downcast_ref::<Interrupted>()
            .expect("the outermost error must be the interrupt marker");
        assert!(
            !marker.after_camera_write,
            "a restored selector is not an unknown camera state"
        );
        assert_eq!(
            io.properties.get(&0xd000),
            Some(&original),
            "the original selector must be restored before the interrupt is honoured"
        );
        assert_eq!(io.writes.last(), Some(&0xd000));
    }

    #[test]
    fn restored_selector_scope_puts_the_camera_back_after_a_successful_write() {
        use crate::interrupt::InterruptLatch;

        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let latch = InterruptLatch::new();
        let mut io = FakeSimulationPropertyIo::with_profile(&profile);
        let original = codec::encode(&9_u16).expect("test selector");
        io.properties.insert(0xd000, original.clone());

        let outcome = with_restored_simulation_selector_io(&latch, &mut io, 0xd000, |io| {
            assert!(latch.is_deferring(), "a write scope is a critical region");
            let mut selected = SelectedSimulationIo::new(io, TestSelector(3));
            selected.set_prop(0xd001, &7_u16).map_err(|error| {
                SimulationTransactionError::preparation(true, error.into_cause())
            })?;
            Ok(SimulationTransactionSuccess::AppliedAndVerified)
        })
        .expect("the write must succeed and the selector must be restored");

        assert_eq!(outcome, SimulationTransactionSuccess::AppliedAndVerified);
        assert!(!latch.is_deferring());
        assert_eq!(io.properties.get(&0xd000), Some(&original));
        assert_eq!(io.writes, [0xd000, 0xd001, 0xd000]);
    }

    #[test]
    fn restored_selector_scope_keeps_the_transaction_outcome_when_restore_succeeds() {
        use crate::interrupt::InterruptLatch;

        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&profile);
        let original = codec::encode(&9_u16).expect("test selector");
        io.properties.insert(0xd000, original.clone());

        let error = with_restored_simulation_selector_io::<_, ()>(
            &InterruptLatch::new(),
            &mut io,
            0xd000,
            |io| {
                let mut selected = SelectedSimulationIo::new(io, TestSelector(3));
                let _: u16 = selected
                    .get_prop(0xd001)
                    .map_err(|error| SimulationTransactionError::preparation(true, error))?;
                Err(SimulationTransactionError::preparation(
                    true,
                    anyhow::anyhow!("rejected before any write"),
                ))
            },
        )
        .expect_err("the operation failure must be returned");

        assert_eq!(error.state(), SimulationFailureState::RejectedWithoutChange);
        assert!(error.selector_restore_error().is_none());
        assert_eq!(io.properties.get(&0xd000), Some(&original));
    }

    #[test]
    fn failed_selector_restore_after_a_write_is_an_unknown_camera_state() {
        use crate::interrupt::InterruptLatch;

        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        // The selector snapshot read succeeds, the target slot is selected,
        // then the restore write of the original selector is rejected.
        let mut io = FakeSimulationPropertyIo::with_profile(&profile).reject_write(0xd000);
        io.properties
            .insert(0xd000, codec::encode(&9_u16).expect("test selector"));

        let error =
            with_restored_simulation_selector_io(&InterruptLatch::new(), &mut io, 0xd000, |_io| {
                Ok(SimulationTransactionSuccess::AppliedAndVerified)
            })
            .expect_err("a write whose selector cannot be restored must not report success");

        assert_eq!(error.state(), SimulationFailureState::CameraStateUnknown);
        assert_eq!(error.phase(), SimulationTransactionPhase::SelectorRestore);
        assert!(
            error.to_string().contains("SelectorRestore"),
            "the phase must name the selector restore: {error}"
        );
    }

    #[test]
    fn out_of_form_selector_snapshot_is_refused_before_any_write() {
        use crate::interrupt::InterruptLatch;

        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        // The camera reports a slot value the permit's descriptor does not
        // recognize; the snapshot must be refused before `operation` runs
        // and before either the profile write or any selector write.
        let mut io =
            FakeSimulationPropertyIo::with_profile(&profile).reject_selector_validation(0xd000);
        io.properties
            .insert(0xd000, codec::encode(&9_u16).expect("test selector"));

        let error = with_restored_simulation_selector_io::<_, ()>(
            &InterruptLatch::new(),
            &mut io,
            0xd000,
            |_io| panic!("operation must not run when the snapshot fails validation"),
        )
        .expect_err("an out-of-form snapshot must be refused before any write");

        assert_eq!(error.phase(), SimulationTransactionPhase::Preparation);
        assert_eq!(error.state(), SimulationFailureState::RejectedWithoutChange);
        assert!(
            error.cause().to_string().contains("0xd000"),
            "the error must name the selector property code: {error}"
        );
        assert!(
            error
                .cause()
                .to_string()
                .contains("outside the verified descriptor form"),
            "the error must state the snapshot is outside the verified descriptor form: {error}"
        );
        assert!(
            io.writes.is_empty(),
            "zero property writes and zero selector writes must be recorded"
        );
        assert_eq!(
            io.reads,
            [0xd000],
            "only the snapshot read may happen before validation refuses it"
        );
    }

    #[test]
    fn temporary_selector_restores_once_after_failure_in_each_batch_slot() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };

        for failed_slot in 1_u16..=7 {
            let mut io = FakeSimulationPropertyIo::with_profile(&profile);
            io.properties
                .insert(0xd000, codec::encode(&9_u16).expect("test selector"));

            let error = with_temporary_simulation_selector_io::<_, ()>(
                &crate::interrupt::InterruptLatch::new(),
                &mut io,
                0xd000,
                |io| {
                    for slot in 1..=7 {
                        let mut selected = SelectedSimulationIo::new(io, TestSelector(slot));
                        let _: u16 = selected.get_prop(0xd001)?;
                        if slot == failed_slot {
                            anyhow::bail!("injected failure after C{slot}");
                        }
                    }
                    Ok(())
                },
            )
            .expect_err("the injected slot read must fail");
            let error = error
                .downcast_ref::<TemporarySimulationSelectorError>()
                .expect("selector failure must remain typed");

            assert_eq!(
                error.state(),
                TemporarySimulationSelectorState::RestoredAndVerified,
                "failed slot C{failed_slot}"
            );
            assert_eq!(
                codec::decode_exact::<u16>(io.properties.get(&0xd000).expect("selector"))
                    .expect("selector value"),
                9,
                "raw selector snapshot must be restored after C{failed_slot}"
            );
            assert_eq!(
                io.writes
                    .iter()
                    .filter(|property| **property == 0xd000)
                    .count(),
                usize::from(failed_slot) + 1,
                "one select per attempted slot plus one outer restore"
            );
        }
    }

    #[test]
    fn selector_readback_mismatch_prevents_profile_read_and_restores_original() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&profile);
        io.properties
            .insert(0xd000, codec::encode(&9_u16).expect("test selector"));

        let error = with_temporary_simulation_selector_io::<_, ()>(
            &crate::interrupt::InterruptLatch::new(),
            &mut io,
            0xd000,
            |io| {
                io.faults.push_back(Fault::ReturnRead(0xd000, 2));
                let mut selected = SelectedSimulationIo::new(io, TestSelector(1));
                let _: u16 = selected.get_prop(0xd001)?;
                Ok(())
            },
        )
        .expect_err("selector mismatch must fail before the profile read");
        let error = error
            .downcast_ref::<TemporarySimulationSelectorError>()
            .expect("selector failure must remain typed");

        assert_eq!(
            error.state(),
            TemporarySimulationSelectorState::RestoredAndVerified
        );
        assert_eq!(
            io.reads
                .iter()
                .filter(|property| **property == 0xd001)
                .count(),
            0,
            "profile property must not be read after selector mismatch"
        );
        assert_eq!(
            codec::decode_exact::<u16>(io.properties.get(&0xd000).expect("selector"))
                .expect("selector value"),
            9
        );
    }

    #[test]
    fn restore_readback_mismatch_reports_unknown_selector_state() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&profile);
        io.properties
            .insert(0xd000, codec::encode(&9_u16).expect("test selector"));

        let error = with_temporary_simulation_selector_io(
            &crate::interrupt::InterruptLatch::new(),
            &mut io,
            0xd000,
            |io| {
                io.faults.push_back(Fault::ReturnRead(0xd000, 7));
                Ok(())
            },
        )
        .expect_err("restore readback mismatch must fail");
        let error = error
            .downcast_ref::<TemporarySimulationSelectorError>()
            .expect("selector failure must remain typed");

        assert_eq!(error.state(), TemporarySimulationSelectorState::Unknown);
        assert!(error.operation_error().is_none());
        assert!(error.restore_error().is_some());
    }

    #[test]
    fn operation_and_restore_failures_are_both_preserved() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&profile);
        io.properties
            .insert(0xd000, codec::encode(&9_u16).expect("test selector"));

        let error = with_temporary_simulation_selector_io::<_, ()>(
            &crate::interrupt::InterruptLatch::new(),
            &mut io,
            0xd000,
            |io| {
                io.faults.push_back(Fault::RejectWrite(0xd000));
                anyhow::bail!("profile read failed");
            },
        )
        .expect_err("operation and restore must fail");
        let error = error
            .downcast_ref::<TemporarySimulationSelectorError>()
            .expect("selector failure must remain typed");

        assert_eq!(error.state(), TemporarySimulationSelectorState::Unknown);
        assert!(
            error
                .operation_error()
                .is_some_and(|error| error.to_string().contains("profile read failed"))
        );
        assert!(
            error
                .restore_error()
                .is_some_and(|error| error.to_string().contains("selector rejection"))
        );
    }

    #[test]
    fn poisoned_profile_read_skips_selector_restore() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let mut io = FakeSimulationPropertyIo::with_profile(&profile).poison_read(0xd001);
        io.properties
            .insert(0xd000, codec::encode(&9_u16).expect("test selector"));

        let error = with_temporary_simulation_selector_io::<_, ()>(
            &crate::interrupt::InterruptLatch::new(),
            &mut io,
            0xd000,
            |io| {
                let mut selected = SelectedSimulationIo::new(io, TestSelector(1));
                let _: u16 = selected.get_prop(0xd001)?;
                Ok(())
            },
        )
        .expect_err("poisoned profile read must fail");
        let error = error
            .downcast_ref::<TemporarySimulationSelectorError>()
            .expect("selector failure must remain typed");

        assert_eq!(error.state(), TemporarySimulationSelectorState::Unknown);
        assert!(error.restore_error().is_none());
        assert_eq!(
            io.writes
                .iter()
                .filter(|property| **property == 0xd000)
                .count(),
            1,
            "only the target selection may be written on a poisoned session"
        );
    }

    #[test]
    fn out_of_form_selector_snapshot_is_refused_before_the_temporary_switch() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        // The camera reports a slot value the permit's descriptor does not
        // recognize; the snapshot must be refused before the target slot is
        // ever selected and before `operation` runs.
        let mut io =
            FakeSimulationPropertyIo::with_profile(&profile).reject_selector_validation(0xd000);
        io.properties
            .insert(0xd000, codec::encode(&9_u16).expect("test selector"));

        let error = with_temporary_simulation_selector_io::<_, ()>(
            &crate::interrupt::InterruptLatch::new(),
            &mut io,
            0xd000,
            |_io| panic!("operation must not run when the snapshot fails validation"),
        )
        .expect_err("an out-of-form snapshot must be refused before the temporary switch");

        assert!(
            error.to_string().contains("0xd000"),
            "the error must name the selector property code: {error}"
        );
        assert!(
            error
                .to_string()
                .contains("outside the verified descriptor form"),
            "the error must state the snapshot is outside the verified descriptor form: {error}"
        );
        assert!(
            io.writes.is_empty(),
            "zero property writes and zero selector writes must be recorded"
        );
        assert_eq!(
            io.reads,
            [0xd000],
            "only the snapshot read may happen before validation refuses it"
        );
    }

    #[test]
    fn selector_drift_after_a_confirmed_write_can_never_report_verified_recovery() {
        let original = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let candidate = TestProfile {
            first: Some(3),
            second: Some(2),
            third: Some(5),
        };
        let mut raw_io = FakeSimulationPropertyIo::with_profile(&original)
            .switch_selector_before_write(0xd001, 2);
        let mut io = SelectedSimulationIo::new(&mut raw_io, TestSelector(1));

        let error = execute_simulation_transaction(&mut io, &original, &candidate)
            .expect_err("post-write selector verification must detect drift");

        assert_eq!(
            error.state(),
            super::SimulationFailureState::CameraStateUnknown
        );
        assert_eq!(error.journal().len(), 1);
        assert_eq!(error.rollback_journal().len(), 1);
        assert!(
            error
                .cause()
                .to_string()
                .contains("selected simulation slot changed")
        );
    }

    #[test]
    fn selector_drift_after_a_confirmed_write_maps_to_uncertain_binding() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let mut raw_io = FakeSimulationPropertyIo::with_profile(&profile)
            .switch_selector_before_write(0xd001, 2);
        let mut selected = SelectedSimulationIo::new(&mut raw_io, TestSelector(1));

        let error = selected
            .set_prop(0xd001, &3_u16)
            .expect_err("selector drift after the write must be reported");

        assert!(
            error.property_confirmed,
            "the framed PTP write itself accepted the value"
        );
        assert!(
            error.binding_uncertain,
            "drift discovered after a confirmed write is uncertain binding, not unconfirmed"
        );
        assert_eq!(
            raw_io
                .writes
                .iter()
                .filter(|property| **property == 0xd000)
                .count(),
            1,
            "the adapter must not select again just to detect the drift"
        );
    }

    #[test]
    fn selector_drift_after_a_write_forces_reselect_before_the_next_access() {
        let profile = TestProfile {
            first: Some(1),
            second: Some(2),
            third: Some(5),
        };
        let mut raw_io = FakeSimulationPropertyIo::with_profile(&profile)
            .switch_selector_before_write(0xd001, 2);

        {
            let mut selected = SelectedSimulationIo::new(&mut raw_io, TestSelector(1));

            selected
                .set_prop(0xd001, &3_u16)
                .expect_err("selector drift after the write must be reported");

            let value: u16 = selected.get_prop(0xd002).expect(
                "a detected drift must force a fresh select before the next access, \
                 not carry the stale selection forward",
            );
            assert_eq!(
                value, 2,
                "the reselected slot must still read back the expected property value"
            );
        }

        assert_eq!(
            raw_io
                .writes
                .iter()
                .filter(|property| **property == 0xd000)
                .count(),
            2,
            "detected drift must force exactly one fresh select before the next access, \
             on top of the initial select"
        );
        assert_eq!(
            codec::decode_exact::<u16>(raw_io.properties.get(&0xd000).expect("selector"))
                .expect("selector value decodes"),
            1,
            "the reselect must restore the target slot rather than trust the drifted one, \
             which is what protects the rollback path from writing to the wrong slot"
        );
    }
}
