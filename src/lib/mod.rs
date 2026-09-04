#![forbid(unsafe_code)]
#![doc = r#"
The supported library surface exposes camera operations through validated,
high-level sessions.

```no_run
use fujicli::{Camera, CameraInfo, CameraMode};
use fujicli::features::{
    backup::BackupArtifact,
    render::RenderOutcome,
    simulation::Simulation,
};

let _: Option<Camera> = None;
let _: Option<CameraMode> = None;
let _: Option<Box<dyn CameraInfo>> = None;
let _: Option<BackupArtifact> = None;
let _: Option<RenderOutcome> = None;
let _: Option<Box<dyn Simulation>> = None;
```

Raw transports and implementation SPIs are deliberately not public:

```compile_fail
use fujicli::ptp::Ptp;
```

```compile_fail
use fujicli::features::base::CameraBase;
```

```compile_fail
use fujicli::features::simulation::CameraSimulationManager;
```

```compile_fail
use fujicli::features::render::CameraRenderManager;
```

```compile_fail
use fujicli::features::simulation::Simulation;

fn denied<T: Simulation>() {
    let _ = T::try_pull;
}
```

```compile_fail
fn denied(camera: &fujicli::Camera) {
    let _ = &camera.ptp;
}
```
"#]

mod camera;
pub mod features;
include!(concat!(env!("OUT_DIR"), "/generated_module.rs"));
pub mod input;
pub mod interrupt;
pub mod policy;
#[doc(hidden)]
#[cfg(feature = "reverse-tools")]
pub mod reverse;

pub(crate) use camera::ptp;
pub use camera::{Camera, CameraMode, SupportedCamera, preflight};
pub use features::base::info::{CameraInfo, CameraInfoListItem, DefaultCameraInfo};

// Coverage-guided fuzzing entry points for the PTP wire parsers. Gated
// behind the non-default `fuzzing` feature, which no distributable build
// enables, so shipped binaries keep the documented encapsulation contract:
// `use fujicli::ptp::Ptp;` must still not compile. The descriptor entry is
// an opaque pass/fail wrapper because `DevicePropDesc` and its companion
// types stay crate-private.
#[cfg(feature = "fuzzing")]
pub use ptp::{
    codec::{decode_exact, encode},
    container::ContainerInfo,
    decode_device_prop_desc_for_fuzzing,
    structs::{DeviceInfo, ObjectInfo},
};
