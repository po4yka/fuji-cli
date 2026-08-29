use std::io::{Read, Seek, Write};

use binrw::{BinRead, BinResult, BinWrite, Endian};

use crate::ptp::Ptp;

pub trait SimulationSetting:
    Sized + for<'a> BinRead<Args<'a> = ()> + for<'a> BinWrite<Args<'a> = ()>
{
    fn prop_code() -> u16;

    fn try_push(&self, ptp: &mut Ptp) -> anyhow::Result<()> {
        ptp.set_prop(Self::prop_code(), self)
    }

    fn try_pull(ptp: &mut Ptp) -> anyhow::Result<Self> {
        ptp.get_prop(Self::prop_code())
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
}
