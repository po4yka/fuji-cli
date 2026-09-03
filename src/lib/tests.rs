use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use super::{
    CameraMode, PtpUsbBinding, PtpUsbCandidate, best_effort_close_session,
    ensure_session_safe_to_close, resolve_camera, select_ptp_usb_binding,
};
use crate::policy::{
    EmulationAcknowledgement, LogicalCameraIdentity, ModelBindingKind, PhysicalUsbIdentity,
};

#[test]
fn drop_close_uses_a_short_absolute_deadline() {
    let start = Instant::now();
    let observed_deadline = RefCell::new(None);

    best_effort_close_session(
        true,
        true,
        || start,
        |deadline| {
            observed_deadline.replace(Some(deadline));
            Ok(())
        },
    );

    assert_eq!(
        observed_deadline.into_inner(),
        Some(start + Duration::from_secs(1))
    );
}

#[test]
fn explicitly_closed_camera_is_not_closed_again_by_drop() {
    let close_called = RefCell::new(false);

    best_effort_close_session(false, true, Instant::now, |_| {
        close_called.replace(true);
        Ok(())
    });

    assert!(!close_called.into_inner());
}

#[test]
fn explicit_close_is_rejected_while_the_ptp_session_is_unsafe() {
    let error = ensure_session_safe_to_close(false)
        .expect_err("CloseSession must not be sent while camera processing may still be active");

    assert!(error.to_string().contains("refusing CloseSession"));
}

#[test]
fn emulation_rejects_an_unsupported_physical_usb_device() {
    let error = resolve_camera(
        CameraMode::Emulated {
            vendor: 0x04cb,
            product: 0x02f7,
            acknowledgement: EmulationAcknowledgement::NotProvided,
        },
        PhysicalUsbIdentity {
            vendor_id: 0x1234,
            product_id: 0x5678,
        },
    )
    .err()
    .expect("unsupported physical devices must be rejected before they are opened");

    assert_eq!(
        error.to_string(),
        "--emulate requires a physically connected supported camera"
    );
}

#[test]
fn emulation_keeps_physical_and_logical_identities_distinct() -> anyhow::Result<()> {
    let physical = PhysicalUsbIdentity {
        vendor_id: 0x04cb,
        product_id: 0x02fc,
    };
    let resolved = resolve_camera(
        CameraMode::Emulated {
            vendor: 0x04cb,
            product: 0x02f7,
            acknowledgement: EmulationAcknowledgement::Provided,
        },
        physical,
    )?;

    assert_eq!(physical.product_id, 0x02fc);
    assert_eq!(
        resolved.logical_identity,
        LogicalCameraIdentity {
            vendor_id: 0x04cb,
            product_id: 0x02f7,
        }
    );
    assert_eq!(resolved.binding, ModelBindingKind::Emulated);
    Ok(())
}

#[test]
fn ptp_endpoint_selection_uses_one_complete_alternate_setting() -> anyhow::Result<()> {
    let binding = select_ptp_usb_binding([
        PtpUsbCandidate {
            interface: 0,
            setting: 0,
            bulk_in: vec![],
            bulk_out: vec![],
        },
        PtpUsbCandidate {
            interface: 0,
            setting: 1,
            bulk_in: vec![(0x81, 512)],
            bulk_out: vec![(0x02, 512)],
        },
    ])?;

    assert_eq!(
        binding,
        PtpUsbBinding {
            interface: 0,
            setting: 1,
            bulk_in: 0x81,
            bulk_out: 0x02,
            bulk_in_max_packet_size: 512,
            bulk_out_max_packet_size: 512,
        }
    );
    Ok(())
}

#[test]
fn ptp_endpoint_selection_fails_closed_when_multiple_alternates_are_viable() {
    let error = select_ptp_usb_binding([
        PtpUsbCandidate {
            interface: 0,
            setting: 0,
            bulk_in: vec![(0x81, 512)],
            bulk_out: vec![(0x02, 512)],
        },
        PtpUsbCandidate {
            interface: 0,
            setting: 1,
            bulk_in: vec![(0x83, 1024)],
            bulk_out: vec![(0x04, 1024)],
        },
    ])
    .expect_err("ambiguous PTP alternate settings must not be selected implicitly");

    assert!(error.to_string().contains("multiple complete"));
}

#[test]
fn ptp_endpoint_selection_rejects_duplicate_bulk_endpoints_within_an_alternate() {
    let error = select_ptp_usb_binding([PtpUsbCandidate {
        interface: 0,
        setting: 0,
        bulk_in: vec![(0x81, 512), (0x83, 512)],
        bulk_out: vec![(0x02, 512)],
    }])
    .expect_err("duplicate bulk endpoints must not be selected implicitly");

    assert!(error.to_string().contains("ambiguous bulk endpoints"));
}

#[test]
fn x_t5_render_profile_re_encodes_gated_off_words_verbatim() {
    use crate::generated::{cameras::C_X_T5, renders::XT5RenderProfile};

    let firmware = C_X_T5
        .firmware_capability_profiles
        .iter()
        .find(|profile| profile.firmware == "4.31")
        .expect("the X-T5 4.31 capability profile must be generated");
    let wire = |option: &str, logical: &str| -> i32 {
        firmware
            .write_wire_value(option, logical)
            .unwrap_or_else(|error| panic!("{option}={logical} must be allowed on 4.31: {error}"))
    };
    let first_allowed = |option: &str| -> i32 {
        let capability = firmware
            .option(option)
            .unwrap_or_else(|| panic!("{option} must have a 4.31 capability entry"));
        wire(option, capability.allowed_values[0])
    };

    // A profile as the camera would report it: a colour simulation, so the two
    // monochromatic words are gated off, and a non-Temperature white balance,
    // so the temperature word is gated off. Those three words are deliberately
    // non-zero; the decoder must carry them through untouched.
    const GATED_OFF: [(&str, i32); 3] = [
        ("monochromatic_color_temperature", 0x1234),
        ("monochromatic_color_tint", 0x2345),
        ("white_balance_temperature", 5600),
    ];
    let mut live = XT5RenderProfile::default();
    for (field, word) in [
        ("head_0", 0),
        ("file_type", first_allowed("file_type")),
        ("image_size", first_allowed("image_size")),
        ("image_quality", first_allowed("image_quality")),
        ("exposure_offset", 0),
        ("dynamic_range", wire("dynamic_range", "auto")),
        (
            "dynamic_range_priority",
            wire("dynamic_range_priority", "off"),
        ),
        ("film_simulation", wire("film_simulation", "provia")),
        ("grain_effect", first_allowed("grain_effect")),
        ("color_chrome_effect", first_allowed("color_chrome_effect")),
        ("white_balance_as_shot", 2),
        ("white_balance", wire("white_balance", "auto")),
        ("white_balance_shift_red", 0),
        ("white_balance_shift_blue", 0),
        ("highlight_tone", 0),
        ("shadow_tone", 0),
        ("color", 0),
        ("sharpness", 0),
        // An integer-lookup option: its wire word is the global table value.
        (
            "noise_reduction",
            crate::generated::options::NoiseReduction::Zero as i32,
        ),
        (
            "lens_modulation_optimizer",
            first_allowed("lens_modulation_optimizer"),
        ),
        ("color_space", first_allowed("color_space")),
        ("smooth_skin_effect", first_allowed("smooth_skin_effect")),
        (
            "color_chrome_fx_blue",
            first_allowed("color_chrome_fx_blue"),
        ),
        ("clarity", 0),
        ("teleconverter", first_allowed("teleconverter")),
    ]
    .into_iter()
    .chain(GATED_OFF)
    {
        live.wire_words.insert(field, word);
    }
    let mut bytes = live
        .encode_for_firmware(firmware)
        .expect("a profile made only of wire words must encode");
    // `tail_0` is write-only, so the live read layout is one word shorter.
    bytes.truncate(bytes.len() - 4);

    let decoded = XT5RenderProfile::decode_for_firmware(&bytes, firmware)
        .expect("the synthesized live profile must decode");
    assert!(decoded.monochromatic_color_temperature.is_none());
    assert!(decoded.monochromatic_color_tint.is_none());
    assert!(decoded.white_balance_temperature.is_none());
    for (field, word) in GATED_OFF {
        assert_eq!(decoded.wire_words.get(field), Some(&word), "{field}");
    }

    let re_encoded = decoded
        .encode_for_firmware(firmware)
        .expect("a decoded profile must encode");
    assert_eq!(
        &re_encoded[..bytes.len()],
        &bytes[..],
        "decode followed by encode must reproduce every wire word, including the gated-off ones"
    );
    assert_eq!(
        &re_encoded[bytes.len()..],
        &[0, 0, 0, 0],
        "tail_0 has no live word"
    );
}

#[test]
fn scaled_options_reject_camera_words_outside_their_declared_range_or_step() {
    use crate::{
        generated::options::{Color, HighlightTone},
        ptp::codec::decode_exact,
    };

    // Color: logical -4..=4 at scale 10, so raw -40..=40 in steps of 10.
    let color = decode_exact::<Color>(&40_i16.to_le_bytes()).expect("raw 40 is the maximum");
    assert_eq!(i32::from(color), 4);
    let error = decode_exact::<Color>(&35_i16.to_le_bytes())
        .expect_err("raw 35 is off the scale step and must not truncate to 3");
    assert!(error.to_string().contains("raw step"), "{error}");
    let error = decode_exact::<Color>(&100_i16.to_le_bytes())
        .expect_err("raw 100 is outside the declared range and must not surface as 10");
    assert!(error.to_string().contains("raw range"), "{error}");

    // The float-scaled family shares the same checked constructor.
    decode_exact::<HighlightTone>(&HighlightTone::RAW_MAX.to_le_bytes())
        .expect("the raw maximum is a valid camera word");
    let error = decode_exact::<HighlightTone>(&(HighlightTone::RAW_MAX + 1).to_le_bytes())
        .expect_err("one past the raw maximum must be rejected at the wire boundary");
    assert!(error.to_string().contains("raw range"), "{error}");
}
