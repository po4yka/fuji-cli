#![forbid(unsafe_code)]

mod camera;
pub mod features;
include!(concat!(env!("OUT_DIR"), "/generated_module.rs"));
pub mod input;
pub mod policy;
#[doc(hidden)]
#[cfg(feature = "reverse-tools")]
pub mod reverse;

pub(crate) use camera::ptp;
pub use camera::{Camera, CameraMode, SupportedCamera, preflight};
pub use features::base::info::{CameraInfo, CameraInfoListItem, DefaultCameraInfo};
