use std::fmt::Display;

use strsim::damerau_levenshtein;
use strum::IntoEnumIterator;

/// Normalizes user input before it is matched against generated parse keys:
/// trim, lowercase, and keep only ASCII alphanumerics plus `.`, `-`, and `+`.
///
/// The code generator applies the same rule to every enum id, display name,
/// and alias when it emits `FromStr` (see `clean_input_key` in
/// `crates/codegen/src/common/options/enum/mod.rs`). Keep the two in sync; the
/// generated per-enum round-trip tests fail if they drift.
pub trait CleanAlphanumeric {
    fn clean(&self) -> String;
}

impl<T: AsRef<str>> CleanAlphanumeric for T {
    fn clean(&self) -> String {
        self.as_ref()
            .trim()
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
            .collect()
    }
}

const SIMILARITY_THRESHOLD: usize = 8;

pub trait Choices {
    fn choices() -> Vec<String>;

    fn closest(input: &str) -> Option<String> {
        let input_lower = input.to_lowercase();

        let mut best_score = usize::MAX;
        let mut best_match: Option<String> = None;

        for choice in Self::choices() {
            let dist = damerau_levenshtein(&input_lower, &choice.to_lowercase());
            if dist < best_score {
                best_score = dist;
                best_match = Some(choice.clone());
            }
        }

        if best_score <= SIMILARITY_THRESHOLD {
            best_match
        } else {
            None
        }
    }
}

impl<T> Choices for T
where
    T: IntoEnumIterator + Display,
{
    fn choices() -> Vec<String> {
        T::iter().map(|v| v.to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Choices, CleanAlphanumeric};
    use strsim::damerau_levenshtein;

    #[derive(Debug, PartialEq, Eq, strum_macros::Display, strum_macros::EnumIter)]
    enum Probe {
        #[strum(serialize = "aaa")]
        Aaa,
        #[strum(serialize = "bbb")]
        Bbb,
        #[strum(serialize = "abcdefghij")]
        LongName,
    }

    #[derive(Debug, PartialEq, Eq, strum_macros::Display, strum_macros::EnumIter)]
    enum BoundaryProbe {
        #[strum(serialize = "abc")]
        Abc,
        #[strum(serialize = "abcdefghij")]
        LongName,
    }

    #[derive(Debug, PartialEq, Eq, strum_macros::Display, strum_macros::EnumIter)]
    enum SingleProbe {
        #[strum(serialize = "abc")]
        Abc,
    }

    #[test]
    fn clean_trims_lowercases_and_keeps_only_allowed_characters() {
        assert_eq!("  PrOVia!! ".clean(), "provia");
        assert_eq!("x-t5.2+beta".clean(), "x-t5.2+beta");
        assert_eq!("日本".clean(), "");
    }

    #[test]
    fn closest_returns_the_exact_choice_ignoring_case() {
        // The result is the Display form of the choice, not the lowercased
        // input.
        assert_eq!(<Probe as Choices>::closest("AAA"), Some("aaa".to_string()));
    }

    #[test]
    fn closest_suggests_within_small_edit_distance() {
        // "aab" is one substitution away from "aaa".
        assert_eq!(damerau_levenshtein("aab", "aaa"), 1);
        assert_eq!(<Probe as Choices>::closest("aab"), Some("aaa".to_string()));
    }

    #[test]
    fn closest_boundary_at_threshold_returns_some() {
        // "zzzzzzzz" (8 z) shares no characters with either choice, so its
        // distance is max(8, len): 8 from "abc", 10 from "abcdefghij". The
        // best distance equals SIMILARITY_THRESHOLD (8) and is accepted.
        assert_eq!(damerau_levenshtein("zzzzzzzz", "abc"), 8);
        assert_eq!(
            <BoundaryProbe as Choices>::closest("zzzzzzzz"),
            Some("abc".to_string())
        );
    }

    #[test]
    fn closest_one_past_threshold_returns_none() {
        // "zzzzzzzzz" (9 z) is distance 9 from "abc", one past
        // SIMILARITY_THRESHOLD.
        assert_eq!(damerau_levenshtein("zzzzzzzzz", "abc"), 9);
        assert_eq!(<SingleProbe as Choices>::closest("zzzzzzzzz"), None);
    }

    #[test]
    fn closest_tie_returns_the_first_declared_choice() {
        // "ccc" is distance 3 from both "aaa" and "bbb"; the strict `<`
        // comparison in `closest` keeps the first choice encountered.
        assert_eq!(damerau_levenshtein("ccc", "aaa"), 3);
        assert_eq!(damerau_levenshtein("ccc", "bbb"), 3);
        assert_eq!(<Probe as Choices>::closest("ccc"), Some("aaa".to_string()));
    }

    #[test]
    fn closest_empty_input_still_suggests() {
        // "" is distance 3 from "abc", within the threshold of 8.
        // NOTE: pinned behavior of the generous threshold, not an
        // endorsement: even a fully stripped input gets a suggestion.
        assert_eq!(damerau_levenshtein("", "abc"), 3);
        assert_eq!(
            <SingleProbe as Choices>::closest(""),
            Some("abc".to_string())
        );
    }

    #[test]
    fn choices_lists_display_strings_in_declaration_order() {
        assert_eq!(
            <Probe as Choices>::choices(),
            vec!["aaa", "bbb", "abcdefghij"]
        );
    }
}
