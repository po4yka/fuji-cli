//! Naming evidence a firmware image carries: the debug identifier table and
//! the localized UI text.
//!
//! What this establishes and what it does not:
//!
//! - The identifiers are real and structured. `MSG_VALS_<OPTION>_<VALUE>`
//!   names an option and one of its values, which is a vocabulary for naming
//!   FML variants without guessing.
//! - The UI text is real localized menu text in every language the camera
//!   ships.
//! - Nothing here binds a PTP wire value to either. The image does not record
//!   that link, so a name still has to be tied to a value by a device capture
//!   or vendor documentation. Treat the output as evidence to read, never as
//!   a table to import.
//! - The text is vendor copyrighted. Extract it locally when reversing; do
//!   not commit it and do not ship it inside a binary.

use std::collections::{BTreeMap, BTreeSet};

/// Shortest identifier worth reporting. Below this the table fills with
/// two-letter register names and other noise.
const MIN_IDENTIFIER_LEN: usize = 6;

/// Shortest UI string worth reporting, in characters. Shorter runs are mostly
/// binary data that happens to decode. The floor costs a few genuine
/// two-character CJK labels, which is the right trade for a dump a human has
/// to read.
const MIN_TEXT_LEN: usize = 3;

/// Upper-case C identifiers, grouped by their first underscore-separated
/// component: `UI`, `MSG`, `BKUP`, `ICO`, and so on.
#[derive(Debug, Default)]
pub struct Identifiers {
    families: BTreeMap<String, BTreeSet<String>>,
}

impl Identifiers {
    pub fn scan(&mut self, buffer: &[u8]) {
        let mut current = Vec::new();
        for byte in buffer.iter().chain(std::iter::once(&0)) {
            if byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_' {
                current.push(*byte);
                continue;
            }
            self.accept(&current);
            current.clear();
        }
    }

    /// An identifier must start with a letter, hold at least one underscore
    /// between two components, and not end on one. That is what separates a
    /// symbol from a run of upper-case text or padding.
    fn accept(&mut self, run: &[u8]) {
        if run.len() < MIN_IDENTIFIER_LEN
            || !run[0].is_ascii_uppercase()
            || run.last() == Some(&b'_')
            || !run.windows(2).any(|pair| pair[0] == b'_')
        {
            return;
        }
        let Ok(identifier) = std::str::from_utf8(run) else {
            return;
        };
        if identifier.split('_').any(str::is_empty) {
            return;
        }
        let family = identifier
            .split('_')
            .next()
            .expect("split always yields one component")
            .to_owned();
        self.families
            .entry(family)
            .or_default()
            .insert(identifier.to_owned());
    }

    pub fn families(&self) -> impl Iterator<Item = (&str, &BTreeSet<String>)> {
        self.families
            .iter()
            .map(|(family, members)| (family.as_str(), members))
    }

    pub fn total(&self) -> usize {
        self.families.values().map(BTreeSet::len).sum()
    }

    /// `MSG_VALS_<OPTION>_<VALUE>` split into option and value names. This is
    /// the part a contributor reads when naming FML variants: the option
    /// vocabulary the camera itself uses, and the value names under it.
    pub fn value_vocabulary(&self) -> BTreeMap<String, BTreeSet<String>> {
        let mut vocabulary: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for identifier in self.families.get("MSG").into_iter().flatten() {
            let Some(rest) = identifier.strip_prefix("MSG_VALS_") else {
                continue;
            };
            let Some((option, value)) = rest.split_once('_') else {
                continue;
            };
            if option.is_empty() || value.is_empty() {
                continue;
            }
            vocabulary
                .entry(option.to_owned())
                .or_default()
                .insert(value.to_owned());
        }
        vocabulary
    }
}

/// Writing system of a UI string, used to group the dump by language family.
/// The image does not label its language blocks, so this is a property of the
/// characters, not a claim about which locale a string belongs to.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Script {
    Ascii,
    LatinExtended,
    Greek,
    Cyrillic,
    Japanese,
    Korean,
}

impl Script {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ascii => "ascii",
            Self::LatinExtended => "latin_extended",
            Self::Greek => "greek",
            Self::Cyrillic => "cyrillic",
            Self::Japanese => "japanese",
            Self::Korean => "korean",
        }
    }

    /// The most specific script present, so a mixed string such as a Japanese
    /// label with an ASCII unit is grouped with Japanese.
    fn of(text: &str) -> Self {
        let mut script = Self::Ascii;
        for unit in text.chars().map(u32::from) {
            let found = match unit {
                0x3040..=0x30FF | 0x3001..=0x303F | 0x4E00..=0x9FFF | 0xFF01..=0xFF60 => {
                    Self::Japanese
                }
                0xAC00..=0xD7A3 => Self::Korean,
                0x0400..=0x045F => Self::Cyrillic,
                0x0384..=0x03CE => Self::Greek,
                0x00A1..=0x017F => Self::LatinExtended,
                _ => continue,
            };
            if found > script {
                script = found;
            }
        }
        script
    }

    const fn accepts(unit: u32) -> bool {
        matches!(unit,
            0x0020..=0x007E
                | 0x00A1..=0x017F
                | 0x0384..=0x03CE
                | 0x0400..=0x045F
                | 0x2010..=0x2027
                | 0x3001..=0x303F
                | 0x3040..=0x30FF
                | 0x4E00..=0x9FFF
                | 0xAC00..=0xD7A3
                | 0xFF01..=0xFF60)
    }
}

/// One UI string with the offset it was found at, so a reader can go back to
/// the image and check its surroundings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiString {
    pub offset: usize,
    pub script: Script,
    pub text: String,
}

/// Extracts NUL-terminated UTF-16LE runs. Both the terminator and a preceding
/// NUL are required, which is what a string table looks like and what most
/// binary data does not.
///
/// This is a raw dump, not a clean catalogue: parts of the image decode as
/// text by accident, more often in the dense CJK and Hangul ranges. Read the
/// output, do not import it.
pub fn scan_ui_strings(buffer: &[u8]) -> Vec<UiString> {
    let mut found = Vec::new();
    let mut current = String::new();
    let mut start = 0;
    let mut began_after_nul = true;
    let mut previous_was_nul = true;

    for (index, pair) in buffer.as_chunks::<2>().0.iter().enumerate() {
        let unit = u16::from_le_bytes(*pair);
        if Script::accepts(u32::from(unit)) {
            if current.is_empty() {
                start = index * 2;
                began_after_nul = previous_was_nul;
            }
            if let Some(character) = char::from_u32(u32::from(unit)) {
                current.push(character);
            }
        } else {
            if unit == 0 && began_after_nul && is_text(&current) {
                found.push(UiString {
                    offset: start,
                    script: Script::of(&current),
                    text: std::mem::take(&mut current),
                });
            }
            current.clear();
        }
        previous_was_nul = unit == 0;
    }
    found
}

/// A run is text when it is long enough and not one character repeated.
/// Padding and fill patterns decode as long single-character runs, which is
/// most of what the accidental matches look like.
fn is_text(run: &str) -> bool {
    run.chars().count() >= MIN_TEXT_LEN && run.chars().collect::<BTreeSet<_>>().len() >= 2
}

pub fn script_counts(strings: &[UiString]) -> BTreeMap<Script, usize> {
    let mut counts = BTreeMap::new();
    for string in strings {
        *counts.entry(string.script).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::{Identifiers, Script, scan_ui_strings, script_counts};

    fn identifiers(buffer: &[u8]) -> Identifiers {
        let mut identifiers = Identifiers::default();
        identifiers.scan(buffer);
        identifiers
    }

    #[test]
    fn identifiers_are_grouped_by_family() {
        let found = identifiers(b"\0UI_SETP_FILM_SIM_ACROS\0MSG_VALS_GRAIN_OFF\0BKUP_FILM_SIM\0");

        let families: Vec<&str> = found.families().map(|(family, _)| family).collect();
        assert_eq!(families, vec!["BKUP", "MSG", "UI"]);
        assert_eq!(found.total(), 3);
    }

    #[test]
    fn runs_that_are_not_identifiers_are_rejected() {
        // Too short, no underscore, trailing underscore, empty component,
        // and ordinary upper-case text.
        let found = identifiers(b"AB_C\0ACROS\0TRAILING_\0DOUBLE__BAR\0FILM SIMULATION\0");

        assert_eq!(found.total(), 0);
    }

    #[test]
    fn the_value_vocabulary_splits_option_from_value() {
        let found = identifiers(
            b"MSG_VALS_GRAIN_OFF\0MSG_VALS_GRAIN_LARGE\0MSG_VALS_CLARITY_MINUS5\0MSG_ITEM_OTHER\0",
        );

        let vocabulary = found.value_vocabulary();

        assert_eq!(vocabulary.len(), 2);
        assert_eq!(
            vocabulary["GRAIN"].iter().cloned().collect::<Vec<_>>(),
            vec!["LARGE".to_owned(), "OFF".to_owned()]
        );
        assert_eq!(
            vocabulary["CLARITY"].iter().cloned().collect::<Vec<_>>(),
            vec!["MINUS5".to_owned()]
        );
    }

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn terminated_utf16_text_is_extracted_with_its_script() {
        let mut buffer = vec![0, 0];
        for text in [
            "FILM SIMULATION",
            "ФИЛЬМ",
            "フィルムシミュレーション",
            "화이트밸런스",
        ] {
            buffer.extend(utf16(text));
            buffer.extend([0, 0]);
        }

        let found = scan_ui_strings(&buffer);

        let texts: Vec<&str> = found.iter().map(|string| string.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "FILM SIMULATION",
                "ФИЛЬМ",
                "フィルムシミュレーション",
                "화이트밸런스"
            ]
        );
        let counts = script_counts(&found);
        assert_eq!(counts[&Script::Ascii], 1);
        assert_eq!(counts[&Script::Cyrillic], 1);
        assert_eq!(counts[&Script::Japanese], 1);
        assert_eq!(counts[&Script::Korean], 1);
    }

    #[test]
    fn an_unterminated_or_unanchored_run_is_not_a_string() {
        let mut unterminated = vec![0, 0];
        unterminated.extend(utf16("MENU"));
        assert!(scan_ui_strings(&unterminated).is_empty());

        // Text that starts immediately after a non-NUL, non-text word is data
        // that happens to decode, not a table entry.
        let mut unanchored = vec![0xFF, 0xFF];
        unanchored.extend(utf16("MENU"));
        unanchored.extend([0, 0]);
        assert!(scan_ui_strings(&unanchored).is_empty());
    }

    #[test]
    fn a_repeated_character_run_is_padding_not_text() {
        let mut buffer = vec![0, 0];
        buffer.extend(utf16("ЄЄЄЄЄЄЄЄ"));
        buffer.extend([0, 0]);

        assert!(scan_ui_strings(&buffer).is_empty());
    }

    #[test]
    fn a_label_below_the_length_floor_is_dropped() {
        let mut buffer = vec![0, 0];
        buffer.extend(utf16("画質"));
        buffer.extend([0, 0]);

        assert!(scan_ui_strings(&buffer).is_empty());
    }

    #[test]
    fn a_mixed_string_is_grouped_with_its_most_specific_script() {
        let mut buffer = vec![0, 0];
        buffer.extend(utf16("ISO 12800 感度"));
        buffer.extend([0, 0]);

        let found = scan_ui_strings(&buffer);

        assert_eq!(found[0].script, Script::Japanese);
        assert_eq!(found[0].offset, 2);
    }
}
