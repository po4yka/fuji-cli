use std::io::{Seek, Write};

use binrw::{BinRead, BinResult, BinWrite, Endian, Error};
use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(Debug, Clone, Copy, BinRead, BinWrite)]
#[brw(little)]
pub struct ContainerInfo {
    pub total_len: u32,
    pub kind: ContainerType,
    pub code: ContainerCode,
    pub transaction_id: u32,
}

impl ContainerInfo {
    pub const SIZE: usize =
        size_of::<u32>() + size_of::<u16>() + size_of::<u16>() + size_of::<u32>();

    pub fn new(
        kind: ContainerType,
        code: CommandCode,
        transaction_id: u32,
        payload_len: usize,
    ) -> anyhow::Result<Self> {
        let total_len = Self::SIZE
            .checked_add(payload_len)
            .ok_or_else(|| anyhow::anyhow!("PTP container length overflow"))?;
        let total_len = u32::try_from(total_len)?;
        let code = ContainerCode::Command(code);

        Ok(Self {
            total_len,
            kind,
            code,
            transaction_id,
        })
    }

    pub fn payload_len(&self) -> anyhow::Result<usize> {
        let total_len = usize::try_from(self.total_len)?;
        total_len.checked_sub(Self::SIZE).ok_or_else(|| {
            anyhow::anyhow!("PTP container length {total_len} is smaller than header")
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::ptp::codec::{decode_exact, encode};

    use super::{CommandCode, ContainerCode, ContainerInfo, ContainerType};

    #[test]
    fn rejects_container_length_smaller_than_header() {
        let container = ContainerInfo {
            total_len: (ContainerInfo::SIZE - 1).try_into().unwrap(),
            kind: ContainerType::Data,
            code: ContainerCode::Command(CommandCode::GetObject),
            transaction_id: 1,
        };

        assert!(container.payload_len().is_err());
    }

    #[test]
    fn rejects_payload_length_that_overflows_the_container_length() {
        let result =
            ContainerInfo::new(ContainerType::Data, CommandCode::SendObject, 1, usize::MAX);

        assert!(result.is_err());
    }

    #[test]
    fn binrw_encodes_container_header_in_wire_order() {
        let container = ContainerInfo {
            total_len: 12,
            kind: ContainerType::Command,
            code: ContainerCode::Command(CommandCode::GetDeviceInfo),
            transaction_id: 0x01020304,
        };

        let encoded = encode(&container).expect("container header encoding must succeed");

        assert_eq!(encoded, [12, 0, 0, 0, 1, 0, 1, 0x10, 4, 3, 2, 1]);
    }

    #[test]
    fn binrw_decodes_container_header_in_wire_order() {
        let decoded = decode_exact::<ContainerInfo>(&[12, 0, 0, 0, 1, 0, 1, 0x10, 4, 3, 2, 1])
            .expect("container header decoding must succeed");

        assert_eq!(decoded.total_len, 12);
        assert_eq!(decoded.kind, ContainerType::Command);
        assert_eq!(
            decoded.code,
            ContainerCode::Command(CommandCode::GetDeviceInfo)
        );
        assert_eq!(decoded.transaction_id, 0x01020304);
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive, BinRead, BinWrite)]
#[brw(repr(u16))]
pub enum CommandCode {
    GetDeviceInfo = 0x1001,
    OpenSession = 0x1002,
    CloseSession = 0x1003,
    GetObjectHandles = 0x1007,
    GetObjectInfo = 0x1008,
    GetObject = 0x1009,
    DeleteObject = 0x100B,
    SendObjectInfo = 0x100C,
    SendObject = 0x100D,
    GetDevicePropValue = 0x1015,
    SetDevicePropValue = 0x1016,
    FujiSendObjectInfo = 0x900c,
    FujiSendObject = 0x900d,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive, BinRead, BinWrite)]
#[brw(repr(u16))]
pub enum ResponseCode {
    Undefined = 0x2000,
    Ok = 0x2001,
    GeneralError = 0x2002,
    SessionNotOpen = 0x2003,
    InvalidTransactionId = 0x2004,
    OperationNotSupported = 0x2005,
    ParameterNotSupported = 0x2006,
    IncompleteTransfer = 0x2007,
    InvalidStorageId = 0x2008,
    InvalidObjectHandle = 0x2009,
    DevicePropNotSupported = 0x200A,
    InvalidObjectFormatCode = 0x200B,
    StoreFull = 0x200C,
    ObjectWriteProtected = 0x200D,
    StoreReadOnly = 0x200E,
    AccessDenied = 0x200F,
    NoThumbnailPresent = 0x2010,
    SelfTestFailed = 0x2011,
    PartialDeletion = 0x2012,
    StoreNotAvailable = 0x2013,
    SpecificationByFormatUnsupported = 0x2014,
    NoValidObjectInfo = 0x2015,
    InvalidCodeFormat = 0x2016,
    UnknownVendorCode = 0x2017,
    CaptureAlreadyTerminated = 0x2018,
    DeviceBusy = 0x2019,
    InvalidParentObject = 0x201A,
    InvalidDevicePropFormat = 0x201B,
    InvalidDevicePropValue = 0x201C,
    InvalidParameter = 0x201D,
    SessionAlreadyOpen = 0x201E,
    TransactionCancelled = 0x201F,
    SpecificationOfDestinationUnsupported = 0x2020,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerCode {
    Command(CommandCode),
    Response(ResponseCode),
    Unknown(u16),
}

impl From<ContainerCode> for u16 {
    fn from(code: ContainerCode) -> Self {
        match code {
            ContainerCode::Command(cmd) => cmd.into(),
            ContainerCode::Response(resp) => resp.into(),
            ContainerCode::Unknown(code) => code,
        }
    }
}

impl TryFrom<u16> for ContainerCode {
    type Error = anyhow::Error;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if let Ok(cmd) = CommandCode::try_from(value) {
            return Ok(Self::Command(cmd));
        }

        if let Ok(resp) = ResponseCode::try_from(value) {
            return Ok(Self::Response(resp));
        }

        Ok(Self::Unknown(value))
    }
}

impl BinWrite for ContainerCode {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        u16::from(*self).write_options(writer, endian, ())
    }
}

impl BinRead for ContainerCode {
    type Args<'a> = ();

    fn read_options<R: std::io::Read + Seek>(
        reader: &mut R,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<Self> {
        let position = reader.stream_position()?;
        let value = u16::read_options(reader, endian, ())?;
        Self::try_from(value).map_err(|error| Error::Custom {
            pos: position,
            err: Box::new(error),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive, BinRead, BinWrite)]
#[brw(repr(u16))]
#[repr(u16)]
pub enum ContainerType {
    Command = 1,
    Data = 2,
    Response = 3,
    Event = 4,
}
