pub mod info;

use anyhow::anyhow;
use info::{CameraInfo, DefaultCameraInfo};
use log::debug;

use crate::{
    SupportedCamera,
    features::{
        backup::CameraBackupManager,
        render::CameraRenderManager,
        simulation::{manager::CameraSimulationManager, parser::CameraSimulationParser},
    },
    generated::options::UsbMode,
    ptp::{DevicePropCode, Ptp, option::SimulationSetting},
};

pub(crate) trait CameraBase {
    type Context: rusb::UsbContext;

    fn camera_definition(&self) -> &'static SupportedCamera;

    fn chunk_size_ceiling(&self) -> usize {
        // Default transport ceiling. Runtime policy selects the effective size.
        1024 * 1024
    }

    fn as_backup_manager(&self) -> Option<&dyn CameraBackupManager<Context = Self::Context>> {
        None
    }

    fn as_simulation_parser(&self) -> Option<&dyn CameraSimulationParser> {
        None
    }

    fn as_simulation_manager(
        &self,
    ) -> Option<&dyn CameraSimulationManager<Context = Self::Context>> {
        None
    }

    fn as_render_manager(&self) -> Option<&dyn CameraRenderManager<Context = Self::Context>> {
        None
    }

    // NOTE: Naively assuming that all cameras can get the same info in the same way.
    fn get_info(&self, ptp: &mut Ptp) -> anyhow::Result<Box<dyn CameraInfo>> {
        let info = ptp.get_info()?;

        let mode = ptp.get_prop(UsbMode::prop_code())?;

        let battery_string = ptp
            .get_prop::<crate::ptp::codec::PtpString>(DevicePropCode::FujiBatteryInfo2)?
            .into_inner();

        let battery: u32 = battery_string
            .split(',')
            .next()
            .ok_or_else(|| anyhow!("Failed to parse battery percentage"))?
            .parse()?;
        debug!("Battery percentage: {battery}");

        let repr = DefaultCameraInfo {
            manufacturer: info.manufacturer,
            model: info.model,
            device_version: info.device_version,
            serial_sha256: crate::features::backup::sha256_hex(info.serial_number.as_bytes()),
            mode,
            battery,
        };

        Ok(Box::new(repr))
    }
}

#[cfg(any(test, feature = "reverse-tools"))]
pub(crate) struct UnknownCamera;

#[cfg(any(test, feature = "reverse-tools"))]
pub(crate) const UNKNOWN_CAMERA: SupportedCamera = SupportedCamera {
    name: "Unknown Camera",
    vendor: 0x0000,
    product: 0x0000,
    ptp_identity: None,
    preflight_profiles: &[],
    firmware_capability_profiles: &[],
    camera_factory: || Box::new(UnknownCamera),
};

#[cfg(any(test, feature = "reverse-tools"))]
impl CameraBase for UnknownCamera {
    type Context = rusb::GlobalContext;

    fn camera_definition(&self) -> &'static SupportedCamera {
        &UNKNOWN_CAMERA
    }
}
