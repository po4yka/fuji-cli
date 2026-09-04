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
/// `None` means "the observable was not read" (no operator-declared slot
/// names were given, or the `0xD18D` read or decode failed) -- the caller
/// always resolves that to `Verdict::Ambiguous`, never fabricating a
/// Still/Movie verdict.
///
/// The observable itself is `0xD18D` (`custom_setting_name`, PTP property
/// `prop_codes::CUSTOM_SETTING_NAME`): the operator gives the probed C1-C7
/// slot distinguishable names in the still and movie namespaces on the
/// camera body ahead of time, and after the `0xD18C` write the probe reads
/// `0xD18D` back and compares it against those two declared names via
/// [`NamespaceSignal::from_slot_name`]. This was identified in
/// `docs/contributors/reversing.md`'s "macOS findings (2026-09-04)" and is
/// implemented and unit-tested here against fakes only; it has not yet been
/// run against a physical camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceSignal {
    OnlyStillChanged,
    OnlyMovieChanged,
    Both,
    Neither,
}

impl NamespaceSignal {
    /// Classifies an `0xD18D` readback (`observed`) against the two
    /// operator-declared slot names for the still and movie namespaces. A
    /// pure, total mapping: every combination of matches resolves to exactly
    /// one variant.
    pub fn from_slot_name(observed: &str, still: &str, movie: &str) -> Self {
        match (observed == still, observed == movie) {
            (true, false) => Self::OnlyStillChanged,
            (false, true) => Self::OnlyMovieChanged,
            (true, true) => Self::Both,
            (false, false) => Self::Neither,
        }
    }
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

    #[test]
    fn from_slot_name_matches_still_only() {
        assert_eq!(
            NamespaceSignal::from_slot_name("still-c1", "still-c1", "movie-c1"),
            NamespaceSignal::OnlyStillChanged
        );
    }

    #[test]
    fn from_slot_name_matches_movie_only() {
        assert_eq!(
            NamespaceSignal::from_slot_name("movie-c1", "still-c1", "movie-c1"),
            NamespaceSignal::OnlyMovieChanged
        );
    }

    #[test]
    fn from_slot_name_matches_both_when_names_are_equal() {
        assert_eq!(
            NamespaceSignal::from_slot_name("same-name", "same-name", "same-name"),
            NamespaceSignal::Both
        );
    }

    #[test]
    fn from_slot_name_matches_neither() {
        assert_eq!(
            NamespaceSignal::from_slot_name("unrelated", "still-c1", "movie-c1"),
            NamespaceSignal::Neither
        );
    }
}
