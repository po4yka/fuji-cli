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
