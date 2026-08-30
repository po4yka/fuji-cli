use std::fmt;

use binrw::{BinRead, BinWrite};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationTransactionSuccess {
    AppliedAndVerified,
    NoChangeVerified,
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
}

impl SimulationPropertyIo for crate::ptp::Ptp {
    fn is_healthy(&self) -> bool {
        crate::ptp::Ptp::is_healthy(self)
    }

    fn get_prop<T>(&mut self, code: u16) -> anyhow::Result<T>
    where
        T: for<'a> BinRead<Args<'a> = ()>,
    {
        crate::ptp::Ptp::get_prop(self, code)
    }

    fn set_prop<T>(&mut self, code: u16, value: &T) -> Result<(), SimulationPropertyWriteError>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        crate::ptp::Ptp::set_prop(self, code, value)
            .map_err(SimulationPropertyWriteError::unconfirmed)
    }
}

pub(crate) struct SelectedSimulationIo<'io, IO, Selector> {
    io: &'io mut IO,
    selector: Selector,
}

impl<'io, IO, Selector> SelectedSimulationIo<'io, IO, Selector> {
    pub(crate) fn new(io: &'io mut IO, selector: Selector) -> Self {
        Self { io, selector }
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
        self.select_and_verify()?;
        let value = self.io.get_prop(code)?;
        self.verify_selector()?;
        Ok(value)
    }

    fn set_prop<T>(&mut self, code: u16, value: &T) -> Result<(), SimulationPropertyWriteError>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        self.select_and_verify()
            .map_err(SimulationPropertyWriteError::unconfirmed)?;
        self.io.set_prop(code, value)?;
        self.verify_selector()
            .map_err(SimulationPropertyWriteError::confirmed_with_uncertain_binding)
    }
}

impl<IO, Selector> SelectedSimulationIo<'_, IO, Selector>
where
    IO: SimulationPropertyIo,
    Selector: crate::ptp::option::SimulationSetting + PartialEq,
{
    fn select_and_verify(&mut self) -> anyhow::Result<()> {
        self.io
            .set_prop(Selector::prop_code(), &self.selector)
            .map_err(|error| error.cause)?;
        self.verify_selector()
    }

    fn verify_selector(&mut self) -> anyhow::Result<()> {
        let selected: Selector = self.io.get_prop(Selector::prop_code())?;
        anyhow::ensure!(
            selected == self.selector,
            "selected simulation slot changed during profile property access"
        );
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
            }),
            Ok(_) => Err(SimulationTransactionError {
                state: SimulationFailureState::CameraStateUnknown,
                phase: SimulationTransactionPhase::ApplyReadback,
                cause: anyhow::anyhow!("simulation readback did not match the original profile"),
                rollback_error: None,
                rollback_readback_error: None,
                journal: Vec::new(),
                rollback_journal: Vec::new(),
            }),
            Err(cause) => Err(SimulationTransactionError {
                state: SimulationFailureState::CameraStateUnknown,
                phase: SimulationTransactionPhase::ApplyReadback,
                cause,
                rollback_error: None,
                rollback_readback_error: None,
                journal: Vec::new(),
                rollback_journal: Vec::new(),
            }),
        };
    }

    let mut journal = Vec::new();
    for change in changes {
        if let Err(error) = candidate.push_change(change, io) {
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
            },
            Err(error) => SimulationTransactionError {
                state: SimulationFailureState::CameraStateUnknown,
                phase: SimulationTransactionPhase::RollbackReadback,
                cause,
                rollback_error: None,
                rollback_readback_error: Some(error),
                journal: receipts,
                rollback_journal: Vec::new(),
            },
        };
    }

    let mut rollback_journal = Vec::new();
    let mut rollback_error = None;
    let mut all_restorable = true;
    for change in journal.iter().rev().copied() {
        if !change.restorable {
            all_restorable = false;
            continue;
        }
        match original.push_change(change, io) {
            Ok(()) => rollback_journal.push(change.into()),
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
            phase: SimulationTransactionPhase::RollbackWrite,
            cause,
            rollback_error,
            rollback_readback_error: None,
            journal: receipts,
            rollback_journal,
        };
    }

    match P::pull_from(io) {
        Ok(readback) if readback == *original && all_restorable && !binding_uncertain => {
            SimulationTransactionError {
                state: SimulationFailureState::RollbackVerified,
                phase: primary_phase,
                cause,
                rollback_error,
                rollback_readback_error: None,
                journal: receipts,
                rollback_journal,
            }
        }
        Ok(_) => SimulationTransactionError {
            state: SimulationFailureState::CameraStateUnknown,
            phase: SimulationTransactionPhase::RollbackReadback,
            cause,
            rollback_error,
            rollback_readback_error: None,
            journal: receipts,
            rollback_journal,
        },
        Err(error) => SimulationTransactionError {
            state: SimulationFailureState::CameraStateUnknown,
            phase: SimulationTransactionPhase::RollbackReadback,
            cause,
            rollback_error,
            rollback_readback_error: Some(error),
            journal: receipts,
            rollback_journal,
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
        SelectedSimulationIo, SimulationPropertyChange, SimulationPropertyIo,
        SimulationPropertyWriteError, SimulationTransactionProfile, SimulationTransactionSuccess,
        execute_simulation_transaction,
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
    }

    #[derive(Default)]
    struct FakeSimulationPropertyIo {
        healthy: bool,
        properties: BTreeMap<u16, Vec<u8>>,
        writes: Vec<u16>,
        faults: VecDeque<Fault>,
        reads: Vec<u16>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fault {
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
            }
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
            let encoded = codec::encode(value).map_err(|error| {
                SimulationPropertyWriteError::unconfirmed(anyhow::Error::from(error))
            })?;
            self.properties.insert(code, encoded);
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
        assert!(io.reads.is_empty(), "poisoned sessions must not be read");
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
        assert_eq!(io.writes, [0xd001, 0xd002]);
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
        assert_eq!(io.writes, [0xd001, 0xd002, 0xd002, 0xd001]);
        assert_eq!(error.rollback_journal().len(), 2);
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
        assert!(io.reads.is_empty(), "poisoned rollback must not read back");
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
        assert_eq!(io.reads, [0xd001]);
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
    fn selected_io_reselects_the_target_before_every_profile_access() {
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
        }

        assert_eq!(io.writes, [0xd000, 0xd001, 0xd000]);
        assert_eq!(io.reads, [0xd000, 0xd000, 0xd000, 0xd002, 0xd000]);
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
}
