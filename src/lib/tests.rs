use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

#[cfg(feature = "reverse-tools")]
use super::authorize_reverse_transport;
use super::{CameraMode, PhysicalUsbIdentity, best_effort_close_session, resolve_supported_camera};
use crate::policy::{EmulationAcknowledgement, ModelBindingKind};

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
    let result = resolve_supported_camera(
        PhysicalUsbIdentity {
            vendor: 0x1234,
            product: 0x5678,
        },
        CameraMode::Emulated {
            vendor: 0x04cb,
            product: 0x02f7,
            acknowledgement: EmulationAcknowledgement::Provided,
        },
    );

    assert!(result.is_err());
}

#[test]
fn emulation_preserves_physical_xt5_and_selects_logical_xs20() -> anyhow::Result<()> {
    let physical = PhysicalUsbIdentity {
        vendor: 0x04cb,
        product: 0x02fc,
    };
    let resolved = resolve_supported_camera(
        physical,
        CameraMode::Emulated {
            vendor: 0x04cb,
            product: 0x02f7,
            acknowledgement: EmulationAcknowledgement::Provided,
        },
    )?
    .expect("emulated mode must resolve a logical implementation");
    let physical_definition = super::find_supported(physical)
        .expect("physical X-T5 must remain independently resolvable");

    assert_eq!(physical.product, 0x02fc);
    assert_eq!(physical_definition.name, "FUJIFILM X-T5");
    assert_eq!(resolved.definition.product, 0x02f7);
    assert_eq!(resolved.definition.name, "FUJIFILM X-S20");
    assert_eq!(resolved.binding, ModelBindingKind::Emulated);
    Ok(())
}

#[cfg(feature = "reverse-tools")]
#[test]
fn raw_reverse_transport_is_only_available_to_unknown_sessions() {
    assert!(authorize_reverse_transport(ModelBindingKind::Native).is_err());
    assert!(authorize_reverse_transport(ModelBindingKind::Emulated).is_err());
    assert!(authorize_reverse_transport(ModelBindingKind::Unknown).is_ok());
}
