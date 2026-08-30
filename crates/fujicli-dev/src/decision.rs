//! Pure decision-table logic for the `simulation-namespace` probe. No I/O; see
//! `docs/contributors/reversing.md`, "Design: the `simulation-namespace`
//! Probe" for the decision table this mirrors.

/// The probe's verdict on whether selector `0xD18C` addresses the still or
/// movie custom-setting namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Still,
    Movie,
    Ambiguous,
}

/// The still/movie signal observed after the probe write, if any is known.
/// `None` means "no known observable" -- per `reversing.md`'s open question
/// 1, no PTP property is currently known to distinguish the two namespaces
/// on the wire, so today's callers always pass `None`. The variants below
/// exist so a future confirmed observable can be wired in without inventing
/// one now (a fabricated Still/Movie verdict from an unknown signal is
/// exactly what this module must never produce).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "no observable is known yet (reversing.md open question 1), so production code \
              always passes None; these variants exist for a future confirmed observable and \
              are exercised only by tests until then"
)]
pub enum NamespaceSignal {
    OnlyStillChanged,
    OnlyMovieChanged,
    Both,
    Neither,
}

/// Maps an observed signal to a verdict per the decision table in
/// `reversing.md`. Ambiguous, unreadable, timed-out, or unknown signals all
/// resolve to `Verdict::Ambiguous`; the caller must print
/// `DO NOT RETRY AUTOMATICALLY` and must not act as though the namespace is
/// known.
pub const fn decide(signal: Option<NamespaceSignal>) -> Verdict {
    match signal {
        Some(NamespaceSignal::OnlyStillChanged) => Verdict::Still,
        Some(NamespaceSignal::OnlyMovieChanged) => Verdict::Movie,
        Some(NamespaceSignal::Both | NamespaceSignal::Neither) | None => Verdict::Ambiguous,
    }
}

#[cfg(test)]
mod tests {
    use super::{NamespaceSignal, Verdict, decide};

    #[test]
    fn only_still_changed_resolves_to_still() {
        assert_eq!(
            decide(Some(NamespaceSignal::OnlyStillChanged)),
            Verdict::Still
        );
    }

    #[test]
    fn only_movie_changed_resolves_to_movie() {
        assert_eq!(
            decide(Some(NamespaceSignal::OnlyMovieChanged)),
            Verdict::Movie
        );
    }

    #[test]
    fn both_changed_is_ambiguous() {
        assert_eq!(decide(Some(NamespaceSignal::Both)), Verdict::Ambiguous);
    }

    #[test]
    fn neither_changed_is_ambiguous() {
        assert_eq!(decide(Some(NamespaceSignal::Neither)), Verdict::Ambiguous);
    }

    #[test]
    fn no_known_observable_defaults_to_ambiguous() {
        assert_eq!(decide(None), Verdict::Ambiguous);
    }
}
