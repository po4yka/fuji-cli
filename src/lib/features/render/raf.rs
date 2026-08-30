const RAF_SIGNATURE: &[u8; 16] = b"FUJIFILMCCD-RAW ";
const RAF_CAMERA_MODEL_RANGE: std::ops::Range<usize> = 0x1c..0x3c;
const RAF_MIN_HEADER_BYTES: usize = 0x6c;
const X_T5_MODEL: &[u8] = b"X-T5";

pub fn validate_xt5_raf(data: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        data.starts_with(RAF_SIGNATURE),
        "input does not have the Fujifilm RAF signature"
    );
    let model = data
        .get(RAF_CAMERA_MODEL_RANGE)
        .ok_or_else(|| anyhow::anyhow!("RAF header is truncated before the camera model"))?;
    let model_len = model
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(model.len());
    anyhow::ensure!(
        &model[..model_len] == X_T5_MODEL,
        "RAW conversion requires a Fujifilm X-T5 RAF"
    );
    anyhow::ensure!(
        data.len() >= RAF_MIN_HEADER_BYTES,
        "X-T5 RAF header is truncated"
    );
    validate_region(data, 0x54, "preview image", false)?;
    validate_region(data, 0x5c, "metadata", false)?;
    validate_region(data, 0x64, "RAW image", true)?;
    Ok(())
}

fn validate_region(
    data: &[u8],
    field_offset: usize,
    name: &str,
    required: bool,
) -> anyhow::Result<()> {
    let offset = read_be_u32(data, field_offset)?;
    let length = read_be_u32(data, field_offset + 4)?;
    if offset == 0 && length == 0 {
        anyhow::ensure!(!required, "RAF {name} region is missing");
        return Ok(());
    }
    anyhow::ensure!(
        offset != 0 && length != 0,
        "RAF {name} region has an incomplete offset/length pair"
    );
    anyhow::ensure!(
        usize::try_from(offset)? >= RAF_MIN_HEADER_BYTES,
        "RAF {name} region overlaps the RAF header"
    );

    let end = offset
        .checked_add(length)
        .ok_or_else(|| anyhow::anyhow!("RAF {name} region overflows"))?;
    anyhow::ensure!(
        usize::try_from(end)? <= data.len(),
        "RAF {name} region is outside the input"
    );
    Ok(())
}

fn read_be_u32(data: &[u8], offset: usize) -> anyhow::Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow::anyhow!("RAF offset directory is truncated"))?;
    let bytes: [u8; 4] = bytes.try_into()?;
    Ok(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::{RAF_SIGNATURE, read_be_u32, validate_xt5_raf};

    fn minimal_raf() -> Vec<u8> {
        let mut data = vec![0; 108];
        data[..RAF_SIGNATURE.len()].copy_from_slice(RAF_SIGNATURE);
        data[0x1c..0x20].copy_from_slice(b"X-T5");
        data
    }

    fn minimal_valid_raf() -> Vec<u8> {
        let mut data = minimal_raf();
        data.push(0xaa);
        data[0x64..0x68].copy_from_slice(&108_u32.to_be_bytes());
        data[0x68..0x6c].copy_from_slice(&1_u32.to_be_bytes());
        data
    }

    #[test]
    fn accepts_bounded_xt5_raf_with_raw_image_payload() {
        validate_xt5_raf(&minimal_valid_raf()).expect("bounded X-T5 RAF must be accepted");
    }

    #[test]
    fn rejects_input_without_the_raf_signature() {
        let mut data = minimal_raf();
        data[0] = b'N';

        let error = validate_xt5_raf(&data).expect_err("non-RAF input must be rejected");

        assert!(error.to_string().contains("RAF signature"));
    }

    #[test]
    fn rejects_raf_from_a_different_camera_model() {
        let mut data = minimal_raf();
        data[0x1c..0x20].copy_from_slice(b"X-H2");

        let error = validate_xt5_raf(&data).expect_err("non-X-T5 RAF must be rejected");

        assert!(error.to_string().contains("X-T5"));
    }

    #[test]
    fn rejects_raf_with_an_out_of_bounds_payload_region() {
        let mut data = minimal_raf();
        data[0x54..0x58].copy_from_slice(&100_u32.to_be_bytes());
        data[0x58..0x5c].copy_from_slice(&20_u32.to_be_bytes());

        let error = validate_xt5_raf(&data).expect_err("out-of-bounds RAF region must be rejected");

        assert!(error.to_string().contains("preview image region"));
    }

    #[test]
    fn rejects_header_only_raf_without_raw_image_payload() {
        let data = minimal_raf();

        let error = validate_xt5_raf(&data)
            .expect_err("a RAF header without RAW image bytes must be rejected");

        assert!(error.to_string().contains("RAW image region"));
    }

    #[test]
    fn rejects_raf_region_with_an_incomplete_offset_length_pair() {
        let mut data = minimal_raf();
        data[0x54..0x58].copy_from_slice(&108_u32.to_be_bytes());
        data[0x58..0x5c].copy_from_slice(&0_u32.to_be_bytes());

        let error = validate_xt5_raf(&data)
            .expect_err("a RAF region with an incomplete offset/length pair must be rejected");

        assert!(error.to_string().contains("incomplete offset/length pair"));
    }

    #[test]
    fn rejects_raf_region_overlapping_the_header() {
        let mut data = minimal_valid_raf();
        data[0x54..0x58].copy_from_slice(&4_u32.to_be_bytes());
        data[0x58..0x5c].copy_from_slice(&8_u32.to_be_bytes());

        let error = validate_xt5_raf(&data)
            .expect_err("a RAF region overlapping the header must be rejected");

        assert!(error.to_string().contains("overlaps the RAF header"));
    }

    #[test]
    fn rejects_raf_region_whose_end_overflows() {
        let mut data = minimal_valid_raf();
        data[0x54..0x58].copy_from_slice(&u32::MAX.to_be_bytes());
        data[0x58..0x5c].copy_from_slice(&1_u32.to_be_bytes());

        let error =
            validate_xt5_raf(&data).expect_err("a RAF region whose end overflows must be rejected");

        assert!(error.to_string().contains("overflows"));
    }

    #[test]
    fn read_be_u32_rejects_a_truncated_directory() {
        let error = read_be_u32(&[0u8; 2], 0).expect_err("a truncated directory must be rejected");

        assert!(error.to_string().contains("truncated"));
    }
}
