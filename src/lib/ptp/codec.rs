use std::io::{self, Cursor, Read, Seek, Write};

use binrw::{BinRead, BinResult, BinWrite, Endian, Error};

const MAX_PTP_ARRAY_ELEMENTS: u32 = 1_000_000;
const MAX_PTP_ARRAY_ALLOCATION_BYTES: usize = 64 * 1024 * 1024;

mod fixed_wire_size {
    pub trait Sealed {}
}

pub trait PtpFixedWireSize: fixed_wire_size::Sealed {
    const WIRE_SIZE: usize;
}

macro_rules! impl_fixed_wire_size {
    ($($type:ty),+ $(,)?) => {
        $(
            impl fixed_wire_size::Sealed for $type {}

            impl PtpFixedWireSize for $type {
                const WIRE_SIZE: usize = std::mem::size_of::<Self>();
            }
        )+
    };
}

impl_fixed_wire_size!(u8, i8, u16, i16, u32, i32, u64, i64);

impl<const N: usize> fixed_wire_size::Sealed for [u8; N] {}

impl<const N: usize> PtpFixedWireSize for [u8; N] {
    const WIRE_SIZE: usize = N;
}

#[derive(Debug, PartialEq, Eq)]
pub struct PtpString(String);

#[derive(Debug, PartialEq, Eq)]
pub struct PtpExactString(String);

#[derive(Debug, PartialEq, Eq)]
pub struct PtpArray<T>(Vec<T>);

impl<T> From<Vec<T>> for PtpArray<T> {
    fn from(value: Vec<T>) -> Self {
        Self(value)
    }
}

impl<T> PtpArray<T> {
    pub fn into_inner(self) -> Vec<T> {
        self.0
    }
}

impl<T> BinWrite for PtpArray<T>
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        write_ptp_array(&self.0, writer, endian, ())
    }
}

impl<T> BinRead for PtpArray<T>
where
    T: for<'a> BinRead<Args<'a> = ()> + PtpFixedWireSize,
{
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<Self> {
        let length = u32::read_options(reader, endian, ())?;
        if length > MAX_PTP_ARRAY_ELEMENTS {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("PTP array length {length} exceeds maximum {MAX_PTP_ARRAY_ELEMENTS}"),
            )));
        }
        let length = usize::try_from(length).map_err(|_| {
            Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "PTP array length does not fit this platform",
            ))
        })?;
        let allocation_bytes = length.checked_mul(T::WIRE_SIZE).ok_or_else(|| {
            Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "PTP array allocation size overflow",
            ))
        })?;
        if allocation_bytes > MAX_PTP_ARRAY_ALLOCATION_BYTES {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "PTP array allocation budget exceeded: {allocation_bytes} bytes exceeds {MAX_PTP_ARRAY_ALLOCATION_BYTES}"
                ),
            )));
        }
        let mut values = Vec::new();
        values.try_reserve_exact(length).map_err(|error| {
            Error::Io(io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!("failed to reserve PTP array allocation: {error}"),
            ))
        })?;
        for _ in 0..length {
            values.push(T::read_options(reader, endian, ())?);
        }
        Ok(Self(values))
    }
}

impl From<&str> for PtpExactString {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl PtpExactString {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl BinWrite for PtpExactString {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        let utf16 = self.0.encode_utf16().collect::<Vec<_>>();
        let length = u8::try_from(utf16.len()).map_err(|_| {
            Error::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "exact PTP string exceeds 255 UTF-16 code units",
            ))
        })?;

        length.write_options(writer, endian, ())?;
        utf16.write_options(writer, endian, ())?;
        Ok(())
    }
}

impl BinRead for PtpExactString {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<Self> {
        let length = u8::read_options(reader, endian, ())?;
        let mut utf16 = Vec::with_capacity(usize::from(length));
        for _ in 0..length {
            utf16.push(u16::read_options(reader, endian, ())?);
        }
        let value = String::from_utf16(&utf16).map_err(|_| {
            Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid UTF-16 in exact PTP string",
            ))
        })?;
        Ok(Self(value))
    }
}

impl From<&str> for PtpString {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl PtpString {
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl BinWrite for PtpString {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        let utf16: Vec<u16> = self.0.encode_utf16().collect();
        if utf16.is_empty() {
            return 0u8.write_options(writer, endian, ());
        }
        let len = u8::try_from(utf16.len() + 1).map_err(|_| {
            Error::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PTP string exceeds 254 UTF-16 code units",
            ))
        })?;
        len.write_options(writer, endian, ())?;
        utf16.write_options(writer, endian, ())?;
        0u16.write_options(writer, endian, ())
    }
}

impl BinRead for PtpString {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<Self> {
        let len = u8::read_options(reader, endian, ())?;
        if len == 0 {
            return Ok(Self(String::new()));
        }

        let mut utf16 = Vec::with_capacity(usize::from(len) - 1);
        for _ in 0..(len - 1) {
            utf16.push(u16::read_options(reader, endian, ())?);
        }
        let terminator = u16::read_options(reader, endian, ())?;
        if terminator != 0 {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "PTP string terminator must be null",
            )));
        }
        let value = String::from_utf16(&utf16).map_err(|_| {
            Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid UTF-16 in PTP string",
            ))
        })?;
        Ok(Self(value))
    }
}

pub fn write_ptp_string<S, W>(value: &S, writer: &mut W, endian: Endian, (): ()) -> BinResult<()>
where
    S: AsRef<str> + ?Sized,
    W: Write + Seek,
{
    PtpString(value.as_ref().to_owned()).write_options(writer, endian, ())
}

pub fn read_ptp_string<R: Read + Seek>(
    reader: &mut R,
    endian: Endian,
    (): (),
) -> BinResult<String> {
    let PtpString(value) = PtpString::read_options(reader, endian, ())?;
    Ok(value)
}

pub fn write_ptp_array<T, C, W>(value: &C, writer: &mut W, endian: Endian, (): ()) -> BinResult<()>
where
    T: for<'a> BinWrite<Args<'a> = ()>,
    C: AsRef<[T]> + ?Sized,
    W: Write + Seek,
{
    let value = value.as_ref();
    let length = u32::try_from(value.len()).map_err(|_| {
        Error::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PTP array length exceeds u32 wire representation",
        ))
    })?;
    length.write_options(writer, endian, ())?;
    for element in value {
        element.write_options(writer, endian, ())?;
    }
    Ok(())
}

pub fn read_ptp_array<T, R>(reader: &mut R, endian: Endian, (): ()) -> BinResult<Vec<T>>
where
    T: for<'a> BinRead<Args<'a> = ()> + PtpFixedWireSize,
    R: Read + Seek,
{
    let PtpArray(values) = PtpArray::<T>::read_options(reader, endian, ())?;
    Ok(values)
}

pub fn encode<T>(value: &T) -> BinResult<Vec<u8>>
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let mut writer = Cursor::new(Vec::new());
    value.write_le(&mut writer)?;
    Ok(writer.into_inner())
}

pub fn decode_exact<T>(bytes: &[u8]) -> BinResult<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    let mut reader = Cursor::new(bytes);
    let value = T::read_le(&mut reader)?;
    if reader.position() != bytes.len() as u64 {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "trailing bytes after PTP value",
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use binrw::{BinRead, BinWrite};

    use super::{
        PtpArray, PtpExactString, PtpString, decode_exact, encode, read_ptp_array, read_ptp_string,
        write_ptp_array, write_ptp_string,
    };

    #[test]
    fn encode_writes_u16_in_little_endian_order() {
        let encoded = encode(&0x1234_u16).expect("u16 encoding must succeed");

        assert_eq!(encoded, [0x34, 0x12]);
    }

    #[test]
    fn decode_exact_rejects_trailing_bytes() {
        let error =
            decode_exact::<u16>(&[0x34, 0x12, 0xff]).expect_err("trailing bytes must be rejected");

        assert!(error.to_string().contains("trailing byte"));
    }

    #[test]
    fn encode_writes_ptp_string_with_length_and_utf16_terminator() {
        let encoded = encode(&PtpString::from("AB")).expect("PTP string encoding must succeed");

        assert_eq!(encoded, [3, 0x41, 0, 0x42, 0, 0, 0]);
    }

    #[test]
    fn encode_writes_empty_ptp_string_as_zero_length_byte() {
        let encoded = encode(&PtpString::from("")).expect("empty PTP string encoding must succeed");

        assert_eq!(encoded, [0]);
    }

    #[test]
    fn decode_reads_ptp_string_with_utf16_terminator() {
        let decoded = decode_exact::<PtpString>(&[3, 0x41, 0, 0x42, 0, 0, 0])
            .expect("PTP string decoding must succeed");

        assert_eq!(decoded, PtpString::from("AB"));
    }

    #[test]
    fn decode_rejects_ptp_string_with_nonzero_terminator() {
        let error = decode_exact::<PtpString>(&[2, 0x41, 0, 0x58, 0])
            .expect_err("nonzero PTP string terminator must be rejected");

        assert!(matches!(
            error,
            binrw::Error::Io(ref error) if error.kind() == std::io::ErrorKind::InvalidData
        ));
    }

    #[test]
    fn encode_writes_exact_ptp_string_without_terminator() {
        let encoded =
            encode(&PtpExactString::from("AB")).expect("exact PTP string encoding must succeed");

        assert_eq!(encoded, [2, 0x41, 0, 0x42, 0]);
    }

    #[test]
    fn decode_reads_exact_ptp_string_without_terminator() {
        let decoded = decode_exact::<PtpExactString>(&[2, 65, 0, 66, 0])
            .expect("exact PTP string decoding must succeed");

        assert_eq!(decoded, PtpExactString::from("AB"));
    }

    #[test]
    fn encode_writes_ptp_array_with_u32_count() {
        let encoded = encode(&PtpArray::from(vec![1u16, 0x0203, 0xffff]))
            .expect("PTP array encoding must succeed");

        assert_eq!(encoded, [3, 0, 0, 0, 1, 0, 3, 2, 255, 255]);
    }

    #[test]
    fn decode_reads_ptp_array_with_u32_count() {
        let decoded = decode_exact::<PtpArray<u16>>(&[3, 0, 0, 0, 1, 0, 3, 2, 255, 255])
            .expect("PTP array decoding must succeed");

        assert_eq!(decoded, PtpArray::from(vec![1u16, 0x0203, 0xffff]));
    }

    #[test]
    fn decode_rejects_ptp_array_count_above_safety_limit() {
        let error = decode_exact::<PtpArray<u8>>(&[0x41, 0x42, 0x0f, 0x00])
            .expect_err("PTP array count above the safety limit must be rejected");

        assert!(error.to_string().contains("exceeds maximum 1000000"));
    }

    #[test]
    fn decode_rejects_ptp_array_rust_allocation_above_byte_budget() {
        let error = decode_exact::<PtpArray<[u8; 1024]>>(&[1, 0, 1, 0])
            .expect_err("PTP arrays must be bounded by allocated bytes as well as element count");

        assert!(error.to_string().contains("allocation budget"));
    }

    #[test]
    fn derived_field_writer_uses_ptp_string_encoding() {
        #[derive(BinWrite)]
        #[bw(little)]
        struct DerivedString {
            #[bw(write_with = write_ptp_string)]
            value: String,
        }

        let encoded = encode(&DerivedString {
            value: "AB".to_owned(),
        })
        .expect("derived PTP string encoding must succeed");

        assert_eq!(encoded, [3, 65, 0, 66, 0, 0, 0]);
    }

    #[test]
    fn derived_field_reader_uses_ptp_string_encoding() {
        #[derive(BinRead, Debug, PartialEq, Eq)]
        #[br(little)]
        struct DerivedString {
            #[br(parse_with = read_ptp_string)]
            value: String,
        }

        let decoded = decode_exact::<DerivedString>(&[3, 65, 0, 66, 0, 0, 0])
            .expect("derived PTP string decoding must succeed");

        assert_eq!(decoded.value, "AB");
    }

    #[test]
    fn derived_field_writer_uses_ptp_array_encoding() {
        #[derive(BinWrite)]
        #[bw(little)]
        struct DerivedArray {
            #[bw(write_with = write_ptp_array)]
            values: Vec<u16>,
        }

        let encoded = encode(&DerivedArray {
            values: vec![1, 0x0203],
        })
        .expect("derived PTP array encoding must succeed");

        assert_eq!(encoded, [2, 0, 0, 0, 1, 0, 3, 2]);
    }

    #[test]
    fn derived_field_reader_uses_ptp_array_encoding() {
        #[derive(BinRead, Debug, PartialEq, Eq)]
        #[br(little)]
        struct DerivedArray {
            #[br(parse_with = read_ptp_array)]
            values: Vec<u16>,
        }

        let decoded = decode_exact::<DerivedArray>(&[2, 0, 0, 0, 1, 0, 3, 2])
            .expect("derived PTP array decoding must succeed");

        assert_eq!(decoded.values, vec![1, 0x0203]);
    }
}
