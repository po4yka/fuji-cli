use std::io::{Read, Seek, Write};

use binrw::{BinRead, BinResult, BinWrite, Endian};

use crate::features::simulation::{SimulationPropertyIo, SimulationPropertyWriteError};

pub(crate) trait SimulationSetting:
    Sized + for<'a> BinRead<Args<'a> = ()> + for<'a> BinWrite<Args<'a> = ()>
{
    fn prop_code() -> u16;

    fn try_push_to<IO: SimulationPropertyIo>(
        &self,
        io: &mut IO,
    ) -> Result<(), SimulationPropertyWriteError> {
        io.set_prop(Self::prop_code(), self)
    }

    fn try_pull_from<IO: SimulationPropertyIo>(io: &mut IO) -> anyhow::Result<Self> {
        io.get_prop(Self::prop_code())
    }
}

pub trait ConversionProfileField: Sized {
    fn write_conversion_profile_field<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
    ) -> BinResult<()>;

    fn read_conversion_profile_field<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
    ) -> BinResult<Self>;

    fn write_conversion_profile_field_for_firmware<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        _profile: &crate::generated::cameras::CameraFirmwareCapabilityProfile,
        _option: &'static str,
    ) -> BinResult<()> {
        self.write_conversion_profile_field(writer, endian)
    }

    fn read_conversion_profile_field_for_firmware<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        _profile: &crate::generated::cameras::CameraFirmwareCapabilityProfile,
        _option: &'static str,
    ) -> BinResult<Self> {
        Self::read_conversion_profile_field(reader, endian)
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, str::FromStr};

    use binrw::{BinRead, BinWrite, Endian};

    use super::{ConversionProfileField, SimulationSetting};
    use crate::{
        features::simulation::{SimulationPropertyIo, SimulationPropertyWriteError},
        generated::{
            cameras::{
                CameraFirmwareCapabilityProfile, CameraOptionCapability, CameraOptionWireValue,
            },
            options::FilmSimulation,
        },
        ptp::codec,
    };

    static WIRE_VALUES: &[CameraOptionWireValue] = &[CameraOptionWireValue {
        logical_value: "provia",
        wire_values: &[0x1234, 0x5678],
    }];
    static OPTIONS: &[CameraOptionCapability] = &[CameraOptionCapability {
        option: "film_simulation",
        allowed_values: &["provia"],
        wire_values: WIRE_VALUES,
    }];
    static PROFILE: CameraFirmwareCapabilityProfile = CameraFirmwareCapabilityProfile {
        firmware: "test",
        options: OPTIONS,
        raw_conversion: None,
    };

    #[derive(Default)]
    struct FirmwareIo(Vec<u8>);

    impl SimulationPropertyIo for FirmwareIo {
        fn is_healthy(&self) -> bool {
            true
        }

        fn get_prop<T>(&mut self, _code: u16) -> anyhow::Result<T>
        where
            T: for<'a> BinRead<Args<'a> = ()>,
        {
            Ok(codec::decode_exact(&self.0)?)
        }

        fn set_prop<T>(&mut self, _code: u16, value: &T) -> Result<(), SimulationPropertyWriteError>
        where
            T: for<'a> BinWrite<Args<'a> = ()>,
        {
            self.0 = codec::encode(value)
                .map_err(|error| SimulationPropertyWriteError::unconfirmed(error.into()))?;
            Ok(())
        }

        fn firmware_option_write_value(
            &self,
            option: &str,
            logical_value: &str,
        ) -> anyhow::Result<i32> {
            PROFILE.write_wire_value(option, logical_value)
        }

        fn firmware_option_read_logical_value(
            &self,
            option: &str,
            wire_value: i32,
        ) -> anyhow::Result<&'static str> {
            PROFILE.read_logical_value(option, wire_value)
        }
    }

    #[test]
    fn simulation_transaction_io_uses_firmware_wire_values() {
        let value = FilmSimulation::from_str("provia").expect("known logical value");
        let mut io = FirmwareIo::default();
        value
            .try_push_to(&mut io)
            .expect("transactional write must use firmware mapping");
        assert_eq!(io.0, 0x1234_u16.to_le_bytes());

        io.0 = 0x5678_u16.to_le_bytes().to_vec();
        let decoded = FilmSimulation::try_pull_from(&mut io)
            .expect("transactional read must accept firmware alias");
        assert_eq!(decoded.capability_value_id(), "provia");
    }

    #[test]
    fn enum_raw_codec_uses_firmware_canonical_and_read_alias_values() {
        let value = FilmSimulation::from_str("provia").expect("known logical value");
        let mut encoded = Cursor::new(Vec::new());
        value
            .write_conversion_profile_field_for_firmware(
                &mut encoded,
                Endian::Little,
                &PROFILE,
                "film_simulation",
            )
            .expect("firmware canonical wire value must encode");
        assert_eq!(encoded.into_inner(), 0x1234_i32.to_le_bytes());

        let mut alias = Cursor::new(0x5678_i32.to_le_bytes());
        let decoded = FilmSimulation::read_conversion_profile_field_for_firmware(
            &mut alias,
            Endian::Little,
            &PROFILE,
            "film_simulation",
        )
        .expect("firmware read alias must decode");
        assert_eq!(decoded.capability_value_id(), "provia");
    }
}
