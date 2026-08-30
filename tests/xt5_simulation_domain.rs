use fujicli::generated::cameras::{
    CameraPreflightOperation, CameraPreflightProfileStatus, SUPPORTED,
};

#[test]
fn xt5_simulation_slots_fail_closed_until_their_domain_is_verified() {
    let camera = SUPPORTED
        .iter()
        .find(|camera| camera.name == "FUJIFILM X-T5")
        .expect("X-T5 must be generated");

    for operation in [
        CameraPreflightOperation::SimulationAccess,
        CameraPreflightOperation::SimulationWrite,
    ] {
        let profile = camera
            .preflight_profiles
            .iter()
            .find(|profile| profile.firmware == "4.31" && profile.operation == operation)
            .expect("X-T5 firmware 4.31 must declare the simulation operation explicitly");

        assert_eq!(
            profile.status,
            CameraPreflightProfileStatus::Unverified,
            "X-T5 {operation:?} must fail closed until the still/movie slot domain is verified"
        );
    }
}

#[test]
fn xt5_raw_conversion_preflight_does_not_depend_on_the_simulation_slot_selector() {
    let camera = SUPPORTED
        .iter()
        .find(|camera| camera.name == "FUJIFILM X-T5")
        .expect("X-T5 must be generated");
    let profile = camera
        .preflight_profiles
        .iter()
        .find(|profile| {
            profile.firmware == "4.31"
                && profile.operation == CameraPreflightOperation::RawConversion
        })
        .expect("X-T5 firmware 4.31 must declare RAW conversion explicitly");

    assert!(
        !profile
            .required_properties
            .iter()
            .any(|property| property.code == 0xD18C),
        "RAW conversion must not select an ambiguous still/movie simulation slot"
    );
    for required_code in [0xD183, 0xD185] {
        assert!(
            profile
                .required_properties
                .iter()
                .any(|property| property.code == required_code),
            "RAW conversion must retain required property 0x{required_code:04X}"
        );
    }
}
