//! Offline reader for Fujifilm firmware update containers (`FWUP*.DAT`).
//!
//! Nothing here touches a camera: the input is a file the vendor publishes.
//! The container layout and the section compression were derived from the
//! X-T5 4.31 image; see
//! `docs/internals/x-t5-firmware-4.31-static-analysis-2026-09-03.md` for the
//! evidence and for what each field is known to mean.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, ensure};
use clap::Subcommand;
use fujicli::features::backup::sha256_hex;
use serde_json::json;

use crate::{
    output::NewOutput,
    strings::{Identifiers, scan_ui_strings, script_counts},
    surface::{Category, PtpSurface, SurfaceDiff, format_codes},
};

/// Header field sizes by container type, from the `FujiHack` wiki and its
/// patcher. Type 6 covers every X-Processor 4 and 5 body this project knows.
const fn model_code_size(container_type: u32) -> Option<usize> {
    match container_type {
        1 => Some(64),
        2 => Some(128),
        3 | 4 | 5 | 6 | 8 => Some(512),
        _ => None,
    }
}

/// `type` plus the model-code field plus four `u32`s: two version words, a
/// checksum, and a device-type word.
const HEADER_TRAILER_BYTES: usize = 16;

/// Page size every observed section header declares. The `SoC` decompressor
/// works on 16 KiB pages, but the stream itself is continuous.
const SECTION_PAGE_BYTES: u32 = 0x4000;

/// Five `u32`s: uncompressed size, stored size, page count, page size, and a
/// constant 4. The stream starts immediately after them, which is what makes
/// every section decode to exactly its declared size.
const SECTION_HEADER_BYTES: usize = 20;

/// Guards against a malformed or hostile header claiming a huge output.
const MAX_SECTION_OUTPUT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Subcommand)]
pub enum FirmwareCommand {
    /// Report the container header and the compressed sections it holds
    Inspect { input: PathBuf },
    /// Write the bit-inverted flash image and every decompressed section
    Unpack {
        input: PathBuf,
        /// Directory to create; an existing path is never overwritten
        output: PathBuf,
    },
    /// Dump the naming evidence an image carries: the debug identifier table
    /// and the localized UI text
    ///
    /// The output is vendor text and a raw dump that needs review: read it
    /// when naming FML values, never commit or ship it.
    Strings {
        input: PathBuf,
        /// JSON artifact to write; an existing path is never overwritten
        output: NewOutput,
    },
    /// Compare the PTP surface two firmware containers declare
    Diff {
        /// The container the current declarations were derived from
        before: PathBuf,
        /// The container to check for surface changes
        after: PathBuf,
    },
}

#[derive(Debug)]
pub struct FirmwareHeader {
    pub container_type: u32,
    /// Body codes the vendor lists in the model-code field, one per camera
    /// this image may be applied to.
    pub model_codes: Vec<String>,
    /// Version as the camera displays it, from the two version words.
    pub version: String,
    pub checksum_word: u32,
    pub device_type_word: u32,
}

#[derive(Debug)]
pub struct FirmwareSection {
    /// Offset of the section header inside the bit-inverted image.
    pub offset: usize,
    pub stored_bytes: usize,
    pub declared_uncompressed_bytes: usize,
    pub decompressed_bytes: usize,
    pub decompressed_sha256: String,
}

#[derive(Debug)]
pub struct FirmwareReport {
    pub schema_version: u8,
    pub input_bytes: usize,
    pub input_sha256: String,
    pub header: FirmwareHeader,
    pub image_bytes: usize,
    pub image_sha256: String,
    pub sections: Vec<FirmwareSection>,
}

/// The manifest an unpack writes beside the extracted files: everything the
/// reader established about the container, so a later run can be diffed
/// against it without re-deriving the layout.
fn manifest(report: &FirmwareReport, surface: &PtpSurface) -> serde_json::Value {
    json!({
        "schema_version": report.schema_version,
        "input_bytes": report.input_bytes,
        "input_sha256": report.input_sha256,
        "header": {
            "container_type": report.header.container_type,
            "model_codes": report.header.model_codes,
            "version": report.header.version,
            "checksum_word": report.header.checksum_word,
            "device_type_word": report.header.device_type_word,
        },
        "image_bytes": report.image_bytes,
        "image_sha256": report.image_sha256,
        "sections": report.sections.iter().map(|section| json!({
            "offset": section.offset,
            "stored_bytes": section.stored_bytes,
            "declared_uncompressed_bytes": section.declared_uncompressed_bytes,
            "decompressed_bytes": section.decompressed_bytes,
            "decompressed_sha256": section.decompressed_sha256,
        })).collect::<Vec<_>>(),
        "ptp_surface": Category::ALL
            .iter()
            .map(|category| {
                let codes: Vec<String> = surface
                    .codes(*category)
                    .iter()
                    .map(|code| format!("0x{code:04X}"))
                    .collect();
                (category.name().replace(' ', "_"), json!(codes))
            })
            .collect::<serde_json::Map<String, serde_json::Value>>(),
    })
}

/// The bit-inverted flash image plus everything decoded from it. The image is
/// kept beside the report so `unpack` can write it without a second pass.
struct Firmware {
    report: FirmwareReport,
    image: Vec<u8>,
    sections: Vec<Vec<u8>>,
    surface: PtpSurface,
}

fn parse_header(raw: &[u8]) -> anyhow::Result<(FirmwareHeader, usize)> {
    let container_type = u32::from_le_bytes(
        raw.get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .context("firmware file is shorter than its type word")?,
    );
    let code_size = model_code_size(container_type).with_context(|| {
        format!("unknown firmware container type {container_type}; refusing to guess its layout")
    })?;
    let header_size = 4 + code_size + HEADER_TRAILER_BYTES;
    ensure!(
        raw.len() > header_size,
        "firmware file is shorter than its {header_size}-byte header"
    );
    let code = &raw[4..4 + code_size];
    let trailer = &raw[4 + code_size..header_size];
    let word = |index: usize| {
        u32::from_le_bytes(
            trailer[index * 4..index * 4 + 4]
                .try_into()
                .expect("the trailer is exactly four u32 words"),
        )
    };

    Ok((
        FirmwareHeader {
            container_type,
            model_codes: model_codes(code),
            // Both words are displayed in hexadecimal: 4 and 0x31 read "4.31".
            version: format!("{:x}.{:02x}", word(0), word(1)),
            checksum_word: word(2),
            device_type_word: word(3),
        },
        header_size,
    ))
}

/// The model-code field holds ASCII hex that decodes to fixed-width decimal
/// body codes. A field that is not hex is reported verbatim rather than
/// discarded, so an unfamiliar container still yields something to compare.
fn model_codes(code: &[u8]) -> Vec<String> {
    let text: String = code
        .iter()
        .take_while(|byte| byte.is_ascii_graphic())
        .map(|byte| char::from(*byte))
        .collect();
    let Some(decoded) = decode_ascii_hex(&text) else {
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![text]
        };
    };
    decoded
        .as_bytes()
        .chunks(8)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .filter(|chunk| chunk.chars().any(|character| character != '0'))
        .collect()
}

fn decode_ascii_hex(text: &str) -> Option<String> {
    if text.len() < 2 || !text.len().is_multiple_of(2) {
        return None;
    }
    let mut decoded = String::with_capacity(text.len() / 2);
    for pair in text.as_bytes().as_chunks::<2>().0 {
        let byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        if !byte.is_ascii_graphic() {
            return None;
        }
        decoded.push(char::from(byte));
    }
    Some(decoded)
}

/// Section header: uncompressed size, stored size (header included), page
/// count, page size, and a constant that marks the format.
fn section_header(image: &[u8], offset: usize) -> Option<(usize, usize)> {
    let words: [u32; 5] = image
        .get(offset..offset + SECTION_HEADER_BYTES)?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| u32::from_le_bytes(*bytes))
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let [uncompressed, stored, pages, page_size, four] = words;
    if page_size != SECTION_PAGE_BYTES || four != 4 || pages == 0 {
        return None;
    }
    let stored = usize::try_from(stored).ok()?;
    let uncompressed = usize::try_from(uncompressed).ok()?;
    let pages = usize::try_from(pages).ok()?;
    let page = usize::try_from(SECTION_PAGE_BYTES).ok()?;
    let fits_pages = stored > pages.checked_sub(1)? * page && stored <= pages * page;
    if !fits_pages
        || stored <= SECTION_HEADER_BYTES
        || uncompressed <= stored
        || offset + stored > image.len()
    {
        return None;
    }
    Some((uncompressed, stored))
}

/// Byte-oriented LZSS with a 2 KiB window: a byte below 0x80 introduces that
/// many literal bytes, anything else is a two-byte match token whose low
/// nibble is the length and whose remaining eleven bits are the distance.
/// Distance zero emits zeros, because the window starts zeroed.
fn decompress(stream: &[u8], budget: usize) -> anyhow::Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut index = 0;
    while index < stream.len() {
        let control = stream[index];
        if control < 0x80 {
            let literals = usize::from(control);
            let start = index + 1;
            let end = start
                .checked_add(literals)
                .filter(|end| *end <= stream.len())
                .context("literal run runs past the end of the section")?;
            out.extend_from_slice(&stream[start..end]);
            index = end;
        } else {
            let low = *stream
                .get(index + 1)
                .context("match token is truncated at the end of the section")?;
            index += 2;
            let length = usize::from(low & 0x0F);
            let distance = (usize::from(control & 0x7F) << 4) | usize::from(low >> 4);
            if distance == 0 {
                out.resize(out.len() + length, 0);
            } else {
                for _ in 0..length {
                    let byte = out.len().checked_sub(distance).map_or(0, |at| out[at]);
                    out.push(byte);
                }
            }
        }
        ensure!(
            out.len() <= budget,
            "section expands past its declared {budget}-byte size"
        );
    }
    Ok(out)
}

fn read(input: &Path) -> anyhow::Result<Firmware> {
    let raw = fs::read(input)
        .with_context(|| format!("reading firmware container {}", input.display()))?;
    let (header, header_size) = parse_header(&raw)?;
    // The payload is obfuscated with a bitwise NOT and nothing else.
    let image: Vec<u8> = raw[header_size..].iter().map(|byte| !byte).collect();

    let mut sections = Vec::new();
    let mut records = Vec::new();
    let mut offset = 0;
    while offset + SECTION_HEADER_BYTES <= image.len() {
        let Some((declared, stored)) = section_header(&image, offset) else {
            offset += 4;
            continue;
        };
        ensure!(
            declared <= MAX_SECTION_OUTPUT_BYTES,
            "section at 0x{offset:x} declares {declared} bytes, above the {MAX_SECTION_OUTPUT_BYTES}-byte unpack budget"
        );
        let decompressed = decompress(
            &image[offset + SECTION_HEADER_BYTES..offset + stored],
            declared,
        )
        .with_context(|| format!("decompressing the section at 0x{offset:x}"))?;
        // A correct decode reproduces the declared size exactly; a mismatch
        // means the stream was misread, so it is an error, not a warning.
        ensure!(
            decompressed.len() == declared,
            "section at 0x{offset:x} declares {declared} bytes but decoded {}",
            decompressed.len()
        );
        records.push(FirmwareSection {
            offset,
            stored_bytes: stored,
            declared_uncompressed_bytes: declared,
            decompressed_bytes: decompressed.len(),
            decompressed_sha256: sha256_hex(&decompressed),
        });
        sections.push(decompressed);
        // Section headers sit on a four-byte grid, but a stored size need not
        // be a multiple of four; realign so the next header is not stepped over.
        offset = (offset + stored).next_multiple_of(4);
    }

    // The static code lists live in the decompressed sections, but an
    // uncompressed region may carry one too, so both are scanned.
    let mut surface = PtpSurface::default();
    surface.scan(&image);
    for section in &sections {
        surface.scan(section);
    }

    Ok(Firmware {
        report: FirmwareReport {
            schema_version: 1,
            input_bytes: raw.len(),
            input_sha256: sha256_hex(&raw),
            header,
            image_bytes: image.len(),
            image_sha256: sha256_hex(&image),
            sections: records,
        },
        image,
        sections,
        surface,
    })
}

fn report(firmware: &FirmwareReport, surface: &PtpSurface) {
    println!(
        "Container type {}, version {}, {} model code(s)",
        firmware.header.container_type,
        firmware.header.version,
        firmware.header.model_codes.len()
    );
    if !firmware.header.model_codes.is_empty() {
        println!("Model codes: {}", firmware.header.model_codes.join(", "));
    }
    println!(
        "Flash image: {} bytes after the bitwise NOT",
        firmware.image_bytes
    );
    for section in &firmware.sections {
        println!(
            "Section at 0x{:x}: {} stored -> {} bytes (header declares {})",
            section.offset,
            section.stored_bytes,
            section.decompressed_bytes,
            section.declared_uncompressed_bytes
        );
    }
    if firmware.sections.is_empty() {
        println!("No compressed sections recognised; the image is written as it is");
    }
    println!(
        "PTP surface: {} code(s) declared in static lists",
        surface.total()
    );
    for category in Category::ALL {
        let codes = surface.codes(category);
        if !codes.is_empty() {
            println!("  {}: {}", category.name(), codes.len());
        }
    }
}

fn inspect(input: &Path) -> anyhow::Result<()> {
    let firmware = read(input)?;
    report(&firmware.report, &firmware.surface);
    Ok(())
}

/// Dumps every identifier and every UI string the image holds. Both are
/// evidence for naming FML values by hand: the image never binds a name to a
/// PTP wire value, so nothing here can be imported automatically.
fn strings(input: &Path, output: &NewOutput) -> anyhow::Result<()> {
    let firmware = read(input)?;
    let mut identifiers = Identifiers::default();
    let mut ui_strings = Vec::new();
    for buffer in std::iter::once(&firmware.image).chain(&firmware.sections) {
        identifiers.scan(buffer);
        ui_strings.extend(scan_ui_strings(buffer));
    }

    println!(
        "Identifiers: {} in {} families",
        identifiers.total(),
        identifiers.families().count()
    );
    let vocabulary = identifiers.value_vocabulary();
    println!(
        "Value vocabulary: {} option name(s) from MSG_VALS_*",
        vocabulary.len()
    );
    println!(
        "UI strings: {} (raw dump, review before use)",
        ui_strings.len()
    );
    for (script, count) in script_counts(&ui_strings) {
        println!("  {}: {count}", script.name());
    }

    let artifact = json!({
        "schema_version": 1,
        "input_sha256": firmware.report.input_sha256,
        "firmware_version": firmware.report.header.version,
        "identifier_families": identifiers
            .families()
            .map(|(family, members)| (family.to_owned(), json!(members)))
            .collect::<serde_json::Map<String, serde_json::Value>>(),
        "value_vocabulary": vocabulary
            .into_iter()
            .map(|(option, values)| (option, json!(values)))
            .collect::<serde_json::Map<String, serde_json::Value>>(),
        "ui_strings": ui_strings
            .iter()
            .map(|string| json!({
                "offset": string.offset,
                "script": string.script.name(),
                "text": string.text,
            }))
            .collect::<Vec<_>>(),
    });
    let mut json = serde_json::to_vec_pretty(&artifact)?;
    json.push(b'\n');
    output.write_all(&json)
}

/// Regression check between two releases: what the newer container adds to or
/// removes from the PTP surface the older one declares. A change here is what
/// invalidates an FML declaration derived from the older image.
fn diff(before: &Path, after: &Path) -> anyhow::Result<()> {
    let old = read(before)?;
    let new = read(after)?;
    println!(
        "{} version {} -> {} version {}",
        before.display(),
        old.report.header.version,
        after.display(),
        new.report.header.version
    );

    let diff = SurfaceDiff::between(&old.surface, &new.surface);
    if diff.is_empty() {
        println!(
            "PTP surface unchanged: {} code(s) on both sides",
            old.surface.total()
        );
        return Ok(());
    }
    for (category, added, removed) in diff.changes() {
        println!("{}:", category.name());
        if !added.is_empty() {
            println!("  added   {}", format_codes(added));
        }
        if !removed.is_empty() {
            println!("  removed {}", format_codes(removed));
        }
    }
    println!(
        "Re-check every FML declaration derived from {}",
        before.display()
    );
    Ok(())
}

fn unpack(input: &Path, output: &Path) -> anyhow::Result<()> {
    let firmware = read(input)?;
    report(&firmware.report, &firmware.surface);
    fs::create_dir(output).with_context(|| {
        format!(
            "creating the unpack directory {}; an existing path is never overwritten",
            output.display()
        )
    })?;
    fs::write(output.join("image.bin"), &firmware.image)?;
    for (section, record) in firmware.sections.iter().zip(&firmware.report.sections) {
        fs::write(
            output.join(format!("section_{:08x}.bin", record.offset)),
            section,
        )?;
    }
    let mut manifest = serde_json::to_vec_pretty(&manifest(&firmware.report, &firmware.surface))?;
    manifest.push(b'\n');
    fs::write(output.join("manifest.json"), manifest)?;
    println!("Wrote {}", output.display());
    Ok(())
}

pub fn handle(command: &FirmwareCommand) -> anyhow::Result<()> {
    match command {
        FirmwareCommand::Inspect { input } => inspect(input),
        FirmwareCommand::Unpack { input, output } => unpack(input, output),
        FirmwareCommand::Strings { input, output } => strings(input, output),
        FirmwareCommand::Diff { before, after } => diff(before, after),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SECTION_HEADER_BYTES, SECTION_PAGE_BYTES, decode_ascii_hex, decompress, model_codes,
        parse_header, section_header,
    };

    /// A minimal type-6 container: the type word, a 512-byte model-code field
    /// carrying one ASCII-hex body code, then the four trailer words.
    fn container(code_text: &str) -> Vec<u8> {
        let mut raw = 6_u32.to_le_bytes().to_vec();
        let mut code = code_text.as_bytes().to_vec();
        code.resize(512, 0);
        raw.extend_from_slice(&code);
        raw.extend_from_slice(&4_u32.to_le_bytes());
        raw.extend_from_slice(&0x31_u32.to_le_bytes());
        raw.extend_from_slice(&0xdead_beef_u32.to_le_bytes());
        raw.extend_from_slice(&1_u32.to_le_bytes());
        raw.push(0xff);
        raw
    }

    #[test]
    fn a_type_six_header_reports_its_version_and_body_codes() {
        // "00056881" hex-encoded, as the vendor stores it.
        let raw = container("3030303536383831");

        let (header, size) = parse_header(&raw).expect("a type-6 header must parse");

        assert_eq!(size, 4 + 512 + 16);
        assert_eq!(header.container_type, 6);
        assert_eq!(header.version, "4.31");
        assert_eq!(header.model_codes, vec!["00056881".to_owned()]);
        assert_eq!(header.device_type_word, 1);
    }

    #[test]
    fn an_unknown_container_type_is_refused_instead_of_guessed() {
        let mut raw = container("3030303536383831");
        raw[0] = 9;

        let error = parse_header(&raw).expect_err("an unknown layout must not be guessed");

        assert!(error.to_string().contains("refusing to guess"), "{error}");
    }

    #[test]
    fn a_truncated_container_is_rejected_before_indexing() {
        let short = 6_u32.to_le_bytes().to_vec();

        let error = parse_header(&short).expect_err("a header-sized read must be bounds checked");

        assert!(error.to_string().contains("shorter than"), "{error}");
    }

    #[test]
    fn a_field_that_is_not_ascii_hex_is_reported_verbatim() {
        assert_eq!(decode_ascii_hex("zz"), None);
        assert_eq!(model_codes(b"X-T5\0\0"), vec!["X-T5".to_owned()]);
        assert!(model_codes(&[0; 8]).is_empty());
    }

    #[test]
    fn literal_runs_and_match_tokens_reconstruct_the_stream() {
        // Four literals "abcd", then a match of length 4 at distance 4.
        let stream = [4, b'a', b'b', b'c', b'd', 0x80, 0x44];

        let out = decompress(&stream, 64).expect("a well-formed stream must decode");

        assert_eq!(out, b"abcdabcd");
    }

    #[test]
    fn a_zero_distance_token_emits_zeros_and_overlap_repeats_one_byte() {
        let zeros = decompress(&[0x80, 0x03], 64).expect("distance zero emits zeros");
        assert_eq!(zeros, vec![0, 0, 0]);

        // One literal, then a length-3 match at distance 1 repeating it.
        let repeat = decompress(&[1, b'x', 0x80, 0x13], 64).expect("overlapping match must decode");
        assert_eq!(repeat, b"xxxx");
    }

    #[test]
    fn a_truncated_stream_fails_instead_of_returning_partial_output() {
        let literal = decompress(&[9, b'a'], 64).expect_err("a short literal run must fail");
        assert!(literal.to_string().contains("past the end"), "{literal}");

        let token = decompress(&[0x80], 64).expect_err("a half match token must fail");
        assert!(token.to_string().contains("truncated"), "{token}");
    }

    #[test]
    fn expansion_past_the_declared_size_is_refused() {
        let error = decompress(&[4, b'a', b'b', b'c', b'd'], 2)
            .expect_err("output beyond the declared size must be refused");

        assert!(error.to_string().contains("expands past"), "{error}");
    }

    /// A whole container around one section, so the reader can be exercised
    /// end to end without shipping a vendor image. The section must actually
    /// compress, because a header whose output is no larger than its input is
    /// not a section.
    fn container_with_section(declared: u32) -> Vec<u8> {
        // Four literals, then four maximum-length matches at distance four.
        let mut stream = vec![4, b'P', b'T', b'P', b'X'];
        for _ in 0..4 {
            stream.extend_from_slice(&[0x80, 0x4F]);
        }
        let stored = SECTION_HEADER_BYTES + stream.len();

        let mut image = Vec::new();
        image.extend_from_slice(&declared.to_le_bytes());
        image.extend_from_slice(
            &u32::try_from(stored)
                .expect("test section is small")
                .to_le_bytes(),
        );
        image.extend_from_slice(&1_u32.to_le_bytes());
        image.extend_from_slice(&SECTION_PAGE_BYTES.to_le_bytes());
        image.extend_from_slice(&4_u32.to_le_bytes());
        image.extend_from_slice(&stream);

        let mut raw = container("3030303536383831");
        raw.pop();
        raw.extend(image.iter().map(|byte| !byte));
        raw
    }

    fn written(container: &[u8]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().expect("a temporary container");
        std::fs::write(file.path(), container).expect("writing the container");
        file
    }

    #[test]
    fn a_container_round_trips_through_the_reader() {
        // Four literals plus four fifteen-byte matches.
        let file = written(&container_with_section(64));

        let firmware = super::read(file.path()).expect("the container must be readable");

        assert_eq!(firmware.report.header.version, "4.31");
        assert_eq!(firmware.report.sections.len(), 1);
        assert_eq!(firmware.sections[0].len(), 64);
        assert!(firmware.sections[0].starts_with(b"PTPXPTPX"));
        assert_eq!(firmware.report.sections[0].decompressed_bytes, 64);
    }

    #[test]
    fn a_section_that_decodes_short_is_an_error_not_a_warning() {
        // The header claims one byte more than the stream can produce.
        let file = written(&container_with_section(65));

        let Err(error) = super::read(file.path()) else {
            panic!("a size mismatch means the layout was misread")
        };

        assert!(error.to_string().contains("but decoded"), "{error}");
    }

    #[test]
    fn a_section_header_is_recognised_only_with_its_format_constants() {
        let mut image = Vec::new();
        image.extend_from_slice(&100_u32.to_le_bytes()); // uncompressed
        image.extend_from_slice(&40_u32.to_le_bytes()); // stored, header included
        image.extend_from_slice(&1_u32.to_le_bytes()); // pages
        image.extend_from_slice(&0x4000_u32.to_le_bytes()); // page size
        image.extend_from_slice(&4_u32.to_le_bytes()); // format constant
        image.resize(40, 0);

        assert_eq!(section_header(&image, 0), Some((100, 40)));

        // 0x4000 little-endian is 00 40 00 00, so byte 13 carries the size.
        let mut wrong_page = image.clone();
        wrong_page[13] = 0x20;
        assert_eq!(section_header(&wrong_page, 0), None);

        let mut smaller_output = image.clone();
        smaller_output[..4].copy_from_slice(&8_u32.to_le_bytes());
        assert_eq!(section_header(&smaller_output, 0), None);

        let mut wrong_constant = image.clone();
        wrong_constant[16] = 5;
        assert_eq!(section_header(&wrong_constant, 0), None);
    }
}
