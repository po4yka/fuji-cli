//! Skeleton for the gated, state-changing `dangerous-reverse-engineering`
//! probe surface. Every command here is compiled only when that feature is
//! enabled; the read-only `discover` surface in [`crate::reverse`] is
//! unaffected.
//!
//! This module intentionally implements only the command-line contract
//! described by plan 007's Step 2, not the six-step guard sequence from
//! `docs/contributors/reversing.md`'s "Requirements for Any Future Dangerous
//! Probe". The guard sequence's mutating send needs a raw single-property
//! PTP write/read primitive that is not reachable from `fujicli-dev` today
//! (`Ptp::set_prop_raw` is `pub(super)`, and the only path that constructs a
//! `MutationPermit` requires a `Verified` FML preflight profile, which does
//! not and cannot exist yet for this exact probe). Adding that primitive
//! would reopen the mutation surface commit `124aa4f` ("fix: seal raw PTP
//! mutation access") deliberately sealed, so it is a maintainer decision, not
//! something implemented here. See `docs/contributors/reversing.md`'s
//! "Design: the `simulation-namespace` Probe" section.

use anyhow::bail;
use clap::{Subcommand, ValueEnum};

use crate::usb::Location;

/// One explicit C1-C7 custom-setting slot. The guard sequence in
/// `docs/contributors/reversing.md` requires selector experiments to touch
/// only one explicit slot per invocation.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CustomSettingSlot {
    C1,
    C2,
    C3,
    C4,
    C5,
    C6,
    C7,
}

#[derive(Debug, Subcommand)]
pub enum ProbeCommand {
    /// Determine whether selector 0xD18C addresses the still or movie
    /// custom-setting namespace (blocked; see module docs).
    SimulationNamespace {
        /// Explicit C1-C7 slot value the probe would write to 0xD18C
        /// exactly once.
        slot: CustomSettingSlot,
    },
}

pub fn handle(command: &ProbeCommand, _location: Location) -> anyhow::Result<()> {
    match command {
        ProbeCommand::SimulationNamespace { .. } => bail!(
            "blocked: fujicli-dev has no raw PTP property write/read primitive to \
             send this probe's mutating operation; see docs/contributors/reversing.md \
             'Design: the simulation-namespace Probe' for the exact gap and why closing \
             it is a maintainer decision, not something this command can do unilaterally"
        ),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    #[derive(Debug, clap::Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: super::ProbeCommand,
    }

    #[test]
    fn simulation_namespace_requires_an_explicit_slot() {
        let error = TestCli::try_parse_from(["probe", "simulation-namespace"])
            .expect_err("the slot argument must be required, not defaulted");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn simulation_namespace_is_blocked_pending_a_raw_write_primitive() {
        let parsed = TestCli::try_parse_from(["probe", "simulation-namespace", "c1"])
            .expect("a single explicit slot must parse");

        let error = super::handle(&parsed.command, super::Location { bus: 1, address: 2 })
            .expect_err("the probe must refuse to run until the raw primitive exists");

        assert!(error.to_string().contains("blocked"));
    }
}
