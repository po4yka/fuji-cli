//! The PTP surface a firmware image declares, and the difference between two
//! of them.
//!
//! A camera's `GetDeviceInfo` answer is assembled at run time, but the code
//! sets it draws from sit in the image as plain `u16` lists. Extracting them
//! statically lets a new firmware release be compared with the one this
//! project was built against, before a camera is available.

use std::collections::{BTreeMap, BTreeSet};

/// A list shorter than this is noise: any four-byte pattern in compressed or
/// binary data can look like a two-code list. The thresholds are per category
/// because the real lists differ in size by an order of magnitude.
const MIN_OPERATIONS: usize = 8;
const MIN_PROPERTIES: usize = 8;
const MIN_EVENTS: usize = 4;

/// Which PTP code space a list belongs to. A list is accepted only when every
/// entry is in the same space and at least one entry is a standard code, so a
/// run of vendor-looking numbers cannot be mistaken for a list.
///
/// Object formats are deliberately absent. Their vendor space cannot be
/// bounded from the evidence, and the wide range matches ordinary UTF-16 text,
/// which these images are full of: scanning for them yielded thousands of
/// false codes on the X-T5 4.31 image. A regression tool that reports invented
/// changes on every release is worse than one that stays silent about formats.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Category {
    Operations,
    Events,
    Properties,
}

impl Category {
    pub const ALL: [Self; 3] = [Self::Operations, Self::Events, Self::Properties];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Operations => "operations",
            Self::Events => "events",
            Self::Properties => "device properties",
        }
    }

    const fn minimum_len(self) -> usize {
        match self {
            Self::Operations => MIN_OPERATIONS,
            Self::Events => MIN_EVENTS,
            Self::Properties => MIN_PROPERTIES,
        }
    }

    /// Standard-range codes, whose presence proves the list is what it looks
    /// like. Every PTP list this project has seen contains at least one.
    const fn is_standard(self, code: u16) -> bool {
        match self {
            Self::Operations => matches!(code, 0x1001..=0x101F),
            Self::Events => matches!(code, 0x4001..=0x400F),
            Self::Properties => matches!(code, 0x5001..=0x50FF),
        }
    }

    /// Vendor extensions Fujifilm uses in each space.
    const fn is_vendor(self, code: u16) -> bool {
        match self {
            Self::Operations => matches!(code, 0x9001..=0x9FFF),
            Self::Events => matches!(code, 0xC001..=0xC0FF),
            Self::Properties => matches!(code, 0xD001..=0xD3FF),
        }
    }

    const fn accepts(self, code: u16) -> bool {
        self.is_standard(code) || self.is_vendor(code)
    }
}

/// Every code the image lists, by category. The union of all lists found:
/// which list a code came from is a run-time decision the image does not
/// record, so a per-list view would claim more than the evidence supports.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PtpSurface {
    codes: BTreeMap<Category, BTreeSet<u16>>,
}

impl PtpSurface {
    pub fn codes(&self, category: Category) -> &BTreeSet<u16> {
        static EMPTY: BTreeSet<u16> = BTreeSet::new();
        self.codes.get(&category).unwrap_or(&EMPTY)
    }

    pub fn total(&self) -> usize {
        self.codes.values().map(BTreeSet::len).sum()
    }

    /// Scans one buffer for terminated `u16` lists. Both parities are scanned
    /// because a decompressed section is not guaranteed to start on an even
    /// offset, and a list is accepted only when it ends on a terminator.
    pub fn scan(&mut self, buffer: &[u8]) {
        for parity in [0_usize, 1] {
            let Some(words) = buffer.get(parity..) else {
                continue;
            };
            let words: Vec<u16> = words
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            self.scan_words(&words);
        }
    }

    fn scan_words(&mut self, words: &[u16]) {
        for category in Category::ALL {
            let mut start = 0;
            while start < words.len() {
                let mut end = start;
                while end < words.len() && category.accepts(words[end]) {
                    end += 1;
                }
                let run = &words[start..end];
                let terminated = words
                    .get(end)
                    .is_some_and(|code| *code == 0x0000 || *code == 0xFFFF);
                if run.len() >= category.minimum_len()
                    && terminated
                    && run.iter().any(|code| category.is_standard(*code))
                    && unique(run)
                {
                    self.codes.entry(category).or_default().extend(run);
                }
                start = if end == start { start + 1 } else { end + 1 };
            }
        }
    }
}

/// A real list never repeats a code; a repeat means the run is data that
/// happens to fall in range.
fn unique(codes: &[u16]) -> bool {
    codes.iter().collect::<BTreeSet<_>>().len() == codes.len()
}

#[derive(Debug, Default)]
pub struct SurfaceDiff {
    changes: BTreeMap<Category, (Vec<u16>, Vec<u16>)>,
}

impl SurfaceDiff {
    pub fn between(before: &PtpSurface, after: &PtpSurface) -> Self {
        let mut changes = BTreeMap::new();
        for category in Category::ALL {
            let before = before.codes(category);
            let after = after.codes(category);
            let added: Vec<u16> = after.difference(before).copied().collect();
            let removed: Vec<u16> = before.difference(after).copied().collect();
            if !added.is_empty() || !removed.is_empty() {
                changes.insert(category, (added, removed));
            }
        }
        Self { changes }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn changes(&self) -> impl Iterator<Item = (Category, &[u16], &[u16])> {
        self.changes
            .iter()
            .map(|(category, (added, removed))| (*category, added.as_slice(), removed.as_slice()))
    }
}

pub fn format_codes(codes: &[u16]) -> String {
    codes
        .iter()
        .map(|code| format!("0x{code:04X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{Category, PtpSurface, SurfaceDiff, format_codes};

    fn image(codes: &[u16], terminator: u16) -> Vec<u8> {
        let mut bytes = vec![0xAA, 0xAA];
        for code in codes {
            bytes.extend_from_slice(&code.to_le_bytes());
        }
        bytes.extend_from_slice(&terminator.to_le_bytes());
        bytes
    }

    fn surface(bytes: &[u8]) -> PtpSurface {
        let mut surface = PtpSurface::default();
        surface.scan(bytes);
        surface
    }

    #[test]
    fn a_terminated_operation_list_is_recognised_with_its_vendor_codes() {
        let codes: &[u16] = &[
            0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x900C, 0x900D, 0x901D,
        ];

        let found = surface(&image(codes, 0x0000));

        assert_eq!(
            found
                .codes(Category::Operations)
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            codes.to_vec()
        );
        assert!(found.codes(Category::Properties).is_empty());
    }

    #[test]
    fn a_property_list_may_end_on_either_terminator() {
        let codes: &[u16] = &[
            0x5005, 0x5015, 0xD001, 0xD007, 0xD008, 0xD00A, 0xD00B, 0xD00C,
        ];

        for terminator in [0x0000, 0xFFFF] {
            let found = surface(&image(codes, terminator));
            assert_eq!(found.codes(Category::Properties).len(), codes.len());
        }
    }

    #[test]
    fn a_list_of_vendor_codes_alone_is_not_accepted() {
        // No standard code, so this could be any run of numbers.
        let codes: &[u16] = &[
            0xD101, 0xD102, 0xD103, 0xD104, 0xD105, 0xD106, 0xD107, 0xD108,
        ];

        assert!(
            surface(&image(codes, 0x0000))
                .codes(Category::Properties)
                .is_empty()
        );
    }

    #[test]
    fn a_short_or_unterminated_or_repeating_run_is_rejected() {
        let short: &[u16] = &[0x1001, 0x1002, 0x1003];
        assert!(
            surface(&image(short, 0))
                .codes(Category::Operations)
                .is_empty()
        );

        let long: &[u16] = &[
            0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x1008,
        ];
        let mut unterminated = vec![0xAA, 0xAA];
        for code in long {
            unterminated.extend_from_slice(&code.to_le_bytes());
        }
        unterminated.extend_from_slice(&0x1234_u16.to_le_bytes());
        assert!(
            surface(&unterminated)
                .codes(Category::Operations)
                .is_empty()
        );

        let repeating: &[u16] = &[
            0x1001, 0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007,
        ];
        assert!(
            surface(&image(repeating, 0))
                .codes(Category::Operations)
                .is_empty()
        );
    }

    #[test]
    fn an_odd_offset_list_is_still_found() {
        let codes: &[u16] = &[
            0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x1008,
        ];
        let mut odd = vec![0x00];
        odd.extend_from_slice(&image(codes, 0));

        assert_eq!(surface(&odd).codes(Category::Operations).len(), codes.len());
    }

    #[test]
    fn utf16_text_does_not_look_like_a_code_list() {
        // "0123456789ABCDEF" as UTF-16LE: every word lands in 0x3000..0x4000,
        // which is why object formats are not extracted at all.
        let text: Vec<u8> = "0123456789ABCDEF"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .chain([0, 0])
            .collect();

        let found = surface(&text);

        assert_eq!(found.total(), 0, "text must not be read as PTP codes");
    }

    #[test]
    fn a_diff_reports_added_and_removed_codes_per_category() {
        let before = surface(&image(
            &[
                0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x1008,
            ],
            0,
        ));
        let after = surface(&image(
            &[
                0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x9801,
            ],
            0,
        ));

        assert!(SurfaceDiff::between(&before, &before).is_empty());

        let diff = SurfaceDiff::between(&before, &after);
        let changes: Vec<_> = diff
            .changes()
            .map(|(category, added, removed)| {
                (category, format_codes(added), format_codes(removed))
            })
            .collect();

        assert_eq!(
            changes,
            vec![(
                Category::Operations,
                "0x9801".to_owned(),
                "0x1008".to_owned()
            )]
        );
    }
}
