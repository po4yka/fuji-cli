use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use super::{CameraMode, best_effort_close_session, resolve_camera};
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
