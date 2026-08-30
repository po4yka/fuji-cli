pub(crate) fn validate_jpeg(data: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        data.starts_with(&[0xff, 0xd8]),
        "rendered object is not a JPEG"
    );

    let mut offset = 2;
    let mut saw_scan = false;
    let mut frame_components = None;
    while offset < data.len() {
        if data[offset] != 0xff {
            anyhow::ensure!(saw_scan, "JPEG data appeared before the first scan");
            offset += 1;
            continue;
        }

        while data.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *data
            .get(offset)
            .ok_or_else(|| anyhow::anyhow!("JPEG ends inside a marker"))?;
        offset += 1;

        match marker {
            0x00 if saw_scan => {}
            0xd0..=0xd7 | 0x01 => {}
            0xd9 => {
                anyhow::ensure!(saw_scan, "JPEG has no image scan");
                anyhow::ensure!(frame_components.is_some(), "JPEG has no image frame");
                anyhow::ensure!(offset == data.len(), "JPEG has trailing data after EOI");
                return Ok(());
            }
            0xd8 => anyhow::bail!("JPEG contains an unexpected nested SOI marker"),
            _ => {
                let length_bytes = data
                    .get(offset..offset + 2)
                    .ok_or_else(|| anyhow::anyhow!("JPEG segment length is truncated"))?;
                let length = usize::from(u16::from_be_bytes(length_bytes.try_into()?));
                anyhow::ensure!(length >= 2, "JPEG segment length is invalid");
                let end = offset
                    .checked_add(length)
                    .ok_or_else(|| anyhow::anyhow!("JPEG segment length overflows"))?;
                anyhow::ensure!(end <= data.len(), "JPEG segment is truncated");
                if is_start_of_frame(marker) {
                    anyhow::ensure!(frame_components.is_none(), "JPEG has multiple image frames");
                    frame_components = Some(validate_start_of_frame(&data[offset..end])?);
                } else if marker == 0xda {
                    let components = frame_components.ok_or_else(|| {
                        anyhow::anyhow!("JPEG scan appears before its image frame")
                    })?;
                    validate_start_of_scan(&data[offset..end], components)?;
                    saw_scan = true;
                }
                offset = end;
            }
        }
    }

    anyhow::bail!("JPEG is missing its EOI marker")
}

fn is_start_of_frame(marker: u8) -> bool {
    matches!(
        marker,
        0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf
    )
}

fn validate_start_of_frame(segment: &[u8]) -> anyhow::Result<u8> {
    anyhow::ensure!(segment.len() >= 8, "JPEG frame header is truncated");
    let precision = segment[2];
    anyhow::ensure!(
        matches!(precision, 8 | 12),
        "JPEG frame has an unsupported sample precision"
    );
    let height = u16::from_be_bytes([segment[3], segment[4]]);
    let width = u16::from_be_bytes([segment[5], segment[6]]);
    anyhow::ensure!(width > 0 && height > 0, "JPEG frame dimensions are empty");
    let components = segment[7];
    anyhow::ensure!(
        (1..=4).contains(&components),
        "JPEG frame component count is invalid"
    );
    anyhow::ensure!(
        segment.len() == 8 + usize::from(components) * 3,
        "JPEG frame component table length is invalid"
    );
    Ok(components)
}

fn validate_start_of_scan(segment: &[u8], frame_components: u8) -> anyhow::Result<()> {
    anyhow::ensure!(segment.len() >= 6, "JPEG scan header is truncated");
    let scan_components = segment[2];
    anyhow::ensure!(
        scan_components > 0 && scan_components <= frame_components,
        "JPEG scan component count is invalid"
    );
    anyhow::ensure!(
        segment.len() == 6 + usize::from(scan_components) * 2,
        "JPEG scan component table length is invalid"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_jpeg;

    fn minimal_valid_jpeg() -> Vec<u8> {
        vec![
            0xff, 0xd8, // SOI
            0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x01, 0x00, 0x01, 0x03, 0x01, 0x11, 0x00, 0x02,
            0x11, 0x00, 0x03, 0x11, 0x00, // one-pixel SOF0
            0xff, 0xda, 0x00, 0x0c, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3f,
            0x00, // SOS
            0x00, 0xff, 0xd9, // entropy byte and EOI
        ]
    }

    #[test]
    fn accepts_structural_frame_scan_and_eoi() {
        validate_jpeg(&minimal_valid_jpeg()).expect("structural JPEG fixture must be accepted");
    }

    #[test]
    fn rejects_scan_without_a_frame() {
        let jpeg = [0xff, 0xd8, 0xff, 0xda, 0x00, 0x02, 0xff, 0xd9];

        let error = validate_jpeg(&jpeg).expect_err("SOS without SOF must be rejected");

        assert!(error.to_string().contains("frame"));
    }

    #[test]
    fn rejects_jpeg_with_a_truncated_segment() {
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x01, 0x02];

        let error = validate_jpeg(&jpeg).expect_err("truncated JPEG segment must be rejected");

        assert!(error.to_string().contains("segment"));
    }
}
