use std::io::{Cursor, Read};

use anyhow::{Context, anyhow, bail, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DevicePropDataType {
    Int8,
    UInt8,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Int64,
    UInt64,
    Int128,
    UInt128,
    Int8Array,
    UInt8Array,
    Int16Array,
    UInt16Array,
    Int32Array,
    UInt32Array,
    Int64Array,
    UInt64Array,
    Int128Array,
    UInt128Array,
    String,
}

impl DevicePropDataType {
    #[cfg(feature = "reverse-tools")]
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Int8 => "int8",
            Self::UInt8 => "uint8",
            Self::Int16 => "int16",
            Self::UInt16 => "uint16",
            Self::Int32 => "int32",
            Self::UInt32 => "uint32",
            Self::Int64 => "int64",
            Self::UInt64 => "uint64",
            Self::Int128 => "int128",
            Self::UInt128 => "uint128",
            Self::Int8Array => "int8_array",
            Self::UInt8Array => "uint8_array",
            Self::Int16Array => "int16_array",
            Self::UInt16Array => "uint16_array",
            Self::Int32Array => "int32_array",
            Self::UInt32Array => "uint32_array",
            Self::Int64Array => "int64_array",
            Self::UInt64Array => "uint64_array",
            Self::Int128Array => "int128_array",
            Self::UInt128Array => "uint128_array",
            Self::String => "string",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DevicePropValue {
    Int(i128),
    UInt(u128),
    Array(Vec<DevicePropValue>),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DevicePropForm {
    None,
    Range {
        minimum: DevicePropValue,
        maximum: DevicePropValue,
        step: DevicePropValue,
    },
    Enumeration(Vec<DevicePropValue>),
}

impl DevicePropForm {
    #[cfg(feature = "reverse-tools")]
    pub(crate) const fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Range { .. } => "range",
            Self::Enumeration(_) => "enumeration",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DevicePropDesc {
    pub property_code: u16,
    pub data_type: DevicePropDataType,
    pub writable: bool,
    pub factory_default: DevicePropValue,
    pub current: DevicePropValue,
    pub form: DevicePropForm,
}

impl DevicePropDesc {
    pub(crate) fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        let mut reader = Cursor::new(bytes);
        let property_code = read_u16(&mut reader)?;
        let data_type = DevicePropDataType::from_code(read_u16(&mut reader)?)?;
        let writable = match read_u8(&mut reader)? {
            0 => false,
            1 => true,
            value => bail!("invalid PTP device property GetSet value {value}"),
        };
        let factory_default = read_value(&mut reader, data_type)?;
        let current = read_value(&mut reader, data_type)?;
        let form = match read_u8(&mut reader)? {
            0 => DevicePropForm::None,
            1 => DevicePropForm::Range {
                minimum: read_value(&mut reader, data_type)?,
                maximum: read_value(&mut reader, data_type)?,
                step: read_value(&mut reader, data_type)?,
            },
            2 => DevicePropForm::Enumeration(read_enumeration(&mut reader, data_type)?),
            value => bail!("unsupported PTP device property form {value}"),
        };
        ensure!(
            reader.position() == bytes.len() as u64,
            "trailing bytes in PTP device property descriptor"
        );

        let descriptor = Self {
            property_code,
            data_type,
            writable,
            factory_default,
            current,
            form,
        };
        descriptor.validate_declared_values()?;
        Ok(descriptor)
    }

    /// Builds a writable descriptor from a statically declared shape plus the
    /// live `GetDevicePropValue` payload, for cameras that refuse
    /// `GetDevicePropDesc`. The payload must decode exactly as `data_type`
    /// and the decoded value must satisfy `form`; the factory default is
    /// unknown, so the live value stands in for it.
    pub(crate) fn from_static(
        property_code: u16,
        data_type: DevicePropDataType,
        form: DevicePropForm,
        current_bytes: &[u8],
    ) -> anyhow::Result<Self> {
        let mut reader = Cursor::new(current_bytes);
        let current = read_value(&mut reader, data_type).with_context(|| {
            format!(
                "live value of PTP device property 0x{property_code:04x} does not decode as datatype 0x{:04x}",
                data_type.code()
            )
        })?;
        ensure!(
            reader.position() == current_bytes.len() as u64,
            "trailing bytes in the live value of PTP device property 0x{property_code:04x}"
        );
        let descriptor = Self {
            property_code,
            data_type,
            writable: true,
            factory_default: current.clone(),
            current,
            form,
        };
        descriptor.validate_declared_values()?;
        Ok(descriptor)
    }

    pub(crate) fn validate_serialized_candidate(&self, candidate: &[u8]) -> anyhow::Result<()> {
        ensure!(
            self.writable,
            "PTP device property 0x{:04x} is read-only",
            self.property_code
        );
        let mut reader = Cursor::new(candidate);
        let candidate_value = read_value(&mut reader, self.data_type).with_context(|| {
            format!(
                "invalid candidate for PTP device property 0x{:04x}",
                self.property_code
            )
        })?;
        ensure!(
            reader.position() == candidate.len() as u64,
            "trailing bytes in PTP device property candidate"
        );
        match &self.form {
            DevicePropForm::None => {}
            DevicePropForm::Range {
                minimum,
                maximum,
                step,
            } => {
                ensure!(
                    value_is_in_range(&candidate_value, minimum, maximum, step),
                    "candidate for PTP device property 0x{:04x} is outside its declared range or step",
                    self.property_code
                );
            }
            DevicePropForm::Enumeration(values) => {
                ensure!(
                    values.contains(&candidate_value),
                    "candidate for PTP device property 0x{:04x} is not in its declared enumeration",
                    self.property_code
                );
            }
        }
        Ok(())
    }

    fn validate_declared_values(&self) -> anyhow::Result<()> {
        match &self.form {
            DevicePropForm::None => Ok(()),
            DevicePropForm::Range {
                minimum,
                maximum,
                step,
            } => {
                ensure!(
                    value_is_in_range(&self.factory_default, minimum, maximum, step),
                    "factory default for PTP device property 0x{:04x} violates its declared range",
                    self.property_code
                );
                ensure!(
                    value_is_in_range(&self.current, minimum, maximum, step),
                    "current value for PTP device property 0x{:04x} violates its declared range",
                    self.property_code
                );
                Ok(())
            }
            DevicePropForm::Enumeration(values) => {
                ensure!(
                    !values.is_empty(),
                    "PTP device property 0x{:04x} declares an empty enumeration",
                    self.property_code
                );
                ensure!(
                    values.contains(&self.factory_default),
                    "factory default for PTP device property 0x{:04x} is absent from its declared enumeration",
                    self.property_code
                );
                ensure!(
                    values.contains(&self.current),
                    "current value for PTP device property 0x{:04x} is absent from its declared enumeration",
                    self.property_code
                );
                Ok(())
            }
        }
    }
}

fn value_is_in_range(
    candidate: &DevicePropValue,
    minimum: &DevicePropValue,
    maximum: &DevicePropValue,
    step: &DevicePropValue,
) -> bool {
    match (candidate, minimum, maximum, step) {
        (
            DevicePropValue::Int(candidate),
            DevicePropValue::Int(minimum),
            DevicePropValue::Int(maximum),
            DevicePropValue::Int(step),
        ) => {
            *step > 0
                && candidate >= minimum
                && candidate <= maximum
                && candidate
                    .checked_sub(*minimum)
                    .is_some_and(|offset| offset % step == 0)
        }
        (
            DevicePropValue::UInt(candidate),
            DevicePropValue::UInt(minimum),
            DevicePropValue::UInt(maximum),
            DevicePropValue::UInt(step),
        ) => {
            *step > 0
                && candidate >= minimum
                && candidate <= maximum
                && candidate
                    .checked_sub(*minimum)
                    .is_some_and(|offset| offset % step == 0)
        }
        _ => false,
    }
}

impl DevicePropDataType {
    /// Encoded width of a scalar datatype; `None` for strings and arrays.
    pub(crate) const fn scalar_len(self) -> Option<usize> {
        match self {
            Self::Int8 | Self::UInt8 => Some(1),
            Self::Int16 | Self::UInt16 => Some(2),
            Self::Int32 | Self::UInt32 => Some(4),
            Self::Int64 | Self::UInt64 => Some(8),
            Self::Int128 | Self::UInt128 => Some(16),
            _ => None,
        }
    }

    pub(crate) fn code(self) -> u16 {
        match self {
            Self::Int8 => 0x0001,
            Self::UInt8 => 0x0002,
            Self::Int16 => 0x0003,
            Self::UInt16 => 0x0004,
            Self::Int32 => 0x0005,
            Self::UInt32 => 0x0006,
            Self::Int64 => 0x0007,
            Self::UInt64 => 0x0008,
            Self::Int128 => 0x0009,
            Self::UInt128 => 0x000a,
            Self::Int8Array => 0x4001,
            Self::UInt8Array => 0x4002,
            Self::Int16Array => 0x4003,
            Self::UInt16Array => 0x4004,
            Self::Int32Array => 0x4005,
            Self::UInt32Array => 0x4006,
            Self::Int64Array => 0x4007,
            Self::UInt64Array => 0x4008,
            Self::Int128Array => 0x4009,
            Self::UInt128Array => 0x400a,
            Self::String => 0xffff,
        }
    }

    pub(crate) fn from_code(code: u16) -> anyhow::Result<Self> {
        match code {
            0x0001 => Ok(Self::Int8),
            0x0002 => Ok(Self::UInt8),
            0x0003 => Ok(Self::Int16),
            0x0004 => Ok(Self::UInt16),
            0x0005 => Ok(Self::Int32),
            0x0006 => Ok(Self::UInt32),
            0x0007 => Ok(Self::Int64),
            0x0008 => Ok(Self::UInt64),
            0x0009 => Ok(Self::Int128),
            0x000a => Ok(Self::UInt128),
            0x4001 => Ok(Self::Int8Array),
            0x4002 => Ok(Self::UInt8Array),
            0x4003 => Ok(Self::Int16Array),
            0x4004 => Ok(Self::UInt16Array),
            0x4005 => Ok(Self::Int32Array),
            0x4006 => Ok(Self::UInt32Array),
            0x4007 => Ok(Self::Int64Array),
            0x4008 => Ok(Self::UInt64Array),
            0x4009 => Ok(Self::Int128Array),
            0x400a => Ok(Self::UInt128Array),
            0xffff => Ok(Self::String),
            _ => bail!("unsupported PTP device property datatype 0x{code:04x}"),
        }
    }
}

fn read_value(
    reader: &mut Cursor<&[u8]>,
    data_type: DevicePropDataType,
) -> anyhow::Result<DevicePropValue> {
    let value = match data_type {
        DevicePropDataType::Int8 => DevicePropValue::Int(i128::from(read_i8(reader)?)),
        DevicePropDataType::UInt8 => DevicePropValue::UInt(u128::from(read_u8(reader)?)),
        DevicePropDataType::Int16 => DevicePropValue::Int(i128::from(read_i16(reader)?)),
        DevicePropDataType::UInt16 => DevicePropValue::UInt(u128::from(read_u16(reader)?)),
        DevicePropDataType::Int32 => DevicePropValue::Int(i128::from(read_i32(reader)?)),
        DevicePropDataType::UInt32 => DevicePropValue::UInt(u128::from(read_u32(reader)?)),
        DevicePropDataType::Int64 => DevicePropValue::Int(i128::from(read_i64(reader)?)),
        DevicePropDataType::UInt64 => DevicePropValue::UInt(u128::from(read_u64(reader)?)),
        DevicePropDataType::Int128 => DevicePropValue::Int(read_i128(reader)?),
        DevicePropDataType::UInt128 => DevicePropValue::UInt(read_u128(reader)?),
        DevicePropDataType::Int8Array => read_array(reader, DevicePropDataType::Int8)?,
        DevicePropDataType::UInt8Array => read_array(reader, DevicePropDataType::UInt8)?,
        DevicePropDataType::Int16Array => read_array(reader, DevicePropDataType::Int16)?,
        DevicePropDataType::UInt16Array => read_array(reader, DevicePropDataType::UInt16)?,
        DevicePropDataType::Int32Array => read_array(reader, DevicePropDataType::Int32)?,
        DevicePropDataType::UInt32Array => read_array(reader, DevicePropDataType::UInt32)?,
        DevicePropDataType::Int64Array => read_array(reader, DevicePropDataType::Int64)?,
        DevicePropDataType::UInt64Array => read_array(reader, DevicePropDataType::UInt64)?,
        DevicePropDataType::Int128Array => read_array(reader, DevicePropDataType::Int128)?,
        DevicePropDataType::UInt128Array => read_array(reader, DevicePropDataType::UInt128)?,
        DevicePropDataType::String => DevicePropValue::String(read_string(reader)?),
    };
    Ok(value)
}

const fn wire_size(element_type: DevicePropDataType) -> Option<usize> {
    match element_type {
        DevicePropDataType::Int8 | DevicePropDataType::UInt8 => Some(1),
        DevicePropDataType::Int16 | DevicePropDataType::UInt16 => Some(2),
        DevicePropDataType::Int32 | DevicePropDataType::UInt32 => Some(4),
        DevicePropDataType::Int64 | DevicePropDataType::UInt64 => Some(8),
        DevicePropDataType::Int128 | DevicePropDataType::UInt128 => Some(16),
        DevicePropDataType::Int8Array
        | DevicePropDataType::UInt8Array
        | DevicePropDataType::Int16Array
        | DevicePropDataType::UInt16Array
        | DevicePropDataType::Int32Array
        | DevicePropDataType::UInt32Array
        | DevicePropDataType::Int64Array
        | DevicePropDataType::UInt64Array
        | DevicePropDataType::Int128Array
        | DevicePropDataType::UInt128Array
        | DevicePropDataType::String => None,
    }
}

/// Upper bound on the memory one decoded descriptor list may occupy. The wire
/// check below bounds the element count by the payload, but each decoded
/// element is a `DevicePropValue` of `size_of::<DevicePropValue>()` bytes (32
/// on 64-bit targets because of the 128-bit variants), so a byte-typed array
/// would otherwise amplify a payload 32x on allocation.
const MAX_DEVICE_PROP_VALUES_ALLOCATION_BYTES: usize = 16 * 1024 * 1024;

fn reserve_values(count: usize, what: &str) -> anyhow::Result<Vec<DevicePropValue>> {
    let allocation_bytes = count
        .checked_mul(size_of::<DevicePropValue>())
        .ok_or_else(|| anyhow!("PTP device property {what} allocation size overflows"))?;
    ensure!(
        allocation_bytes <= MAX_DEVICE_PROP_VALUES_ALLOCATION_BYTES,
        "PTP device property {what} exceeds the in-memory allocation budget: {allocation_bytes} bytes exceeds {MAX_DEVICE_PROP_VALUES_ALLOCATION_BYTES}"
    );
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|error| anyhow!("failed to reserve PTP device property {what}: {error}"))?;
    Ok(values)
}

fn read_array(
    reader: &mut Cursor<&[u8]>,
    element_type: DevicePropDataType,
) -> anyhow::Result<DevicePropValue> {
    let count = usize::try_from(read_u32(reader)?)?;
    let Some(element_size) = wire_size(element_type) else {
        bail!("PTP device property array element type has no fixed wire size");
    };
    let required_bytes = count
        .checked_mul(element_size)
        .ok_or_else(|| anyhow!("PTP device property array size overflows"))?;
    let remaining_bytes = reader
        .get_ref()
        .len()
        .saturating_sub(usize::try_from(reader.position())?);
    ensure!(
        required_bytes <= remaining_bytes,
        "PTP device property array is larger than its payload"
    );
    let mut values = reserve_values(count, "array")?;
    for _ in 0..count {
        values.push(read_value(reader, element_type)?);
    }
    Ok(DevicePropValue::Array(values))
}

/// PTP allows a string property to enumerate its permitted values, and a
/// camera that answers `GetDevicePropDesc` for one (the battery string among
/// the required properties, for instance) must not fail the whole descriptor
/// decode. A string has no fixed width, but it never encodes in fewer than
/// one byte, so the payload still bounds the count before anything is
/// reserved.
fn read_enumeration(
    reader: &mut Cursor<&[u8]>,
    element_type: DevicePropDataType,
) -> anyhow::Result<Vec<DevicePropValue>> {
    let count = usize::from(read_u16(reader)?);
    let minimum_bytes = match wire_size(element_type) {
        Some(element_size) => count
            .checked_mul(element_size)
            .ok_or_else(|| anyhow!("PTP device property enumeration size overflows"))?,
        None if element_type == DevicePropDataType::String => count,
        None => bail!("PTP device property enumeration element type has no fixed wire size"),
    };
    let remaining_bytes = reader
        .get_ref()
        .len()
        .saturating_sub(usize::try_from(reader.position())?);
    ensure!(
        minimum_bytes <= remaining_bytes,
        "PTP device property enumeration is larger than its payload"
    );
    let mut values = reserve_values(count, "enumeration")?;
    for _ in 0..count {
        values.push(read_value(reader, element_type)?);
    }
    Ok(values)
}

fn read_string(reader: &mut Cursor<&[u8]>) -> anyhow::Result<String> {
    let length = read_u8(reader)?;
    if length == 0 {
        return Ok(String::new());
    }

    let mut utf16 = Vec::with_capacity(usize::from(length) - 1);
    for _ in 0..length - 1 {
        utf16.push(read_u16(reader)?);
    }
    ensure!(read_u16(reader)? == 0, "PTP string terminator must be null");
    Ok(String::from_utf16(&utf16)?)
}

fn read_u8(reader: &mut Cursor<&[u8]>) -> anyhow::Result<u8> {
    Ok(read_bytes::<1>(reader)?[0])
}

fn read_i8(reader: &mut Cursor<&[u8]>) -> anyhow::Result<i8> {
    Ok(i8::from_le_bytes(read_bytes(reader)?))
}

fn read_u16(reader: &mut Cursor<&[u8]>) -> anyhow::Result<u16> {
    Ok(u16::from_le_bytes(read_bytes(reader)?))
}

fn read_i16(reader: &mut Cursor<&[u8]>) -> anyhow::Result<i16> {
    Ok(i16::from_le_bytes(read_bytes(reader)?))
}

fn read_u32(reader: &mut Cursor<&[u8]>) -> anyhow::Result<u32> {
    Ok(u32::from_le_bytes(read_bytes(reader)?))
}

fn read_i32(reader: &mut Cursor<&[u8]>) -> anyhow::Result<i32> {
    Ok(i32::from_le_bytes(read_bytes(reader)?))
}

fn read_u64(reader: &mut Cursor<&[u8]>) -> anyhow::Result<u64> {
    Ok(u64::from_le_bytes(read_bytes(reader)?))
}

fn read_i64(reader: &mut Cursor<&[u8]>) -> anyhow::Result<i64> {
    Ok(i64::from_le_bytes(read_bytes(reader)?))
}

fn read_u128(reader: &mut Cursor<&[u8]>) -> anyhow::Result<u128> {
    Ok(u128::from_le_bytes(read_bytes(reader)?))
}

fn read_i128(reader: &mut Cursor<&[u8]>) -> anyhow::Result<i128> {
    Ok(i128::from_le_bytes(read_bytes(reader)?))
}

fn read_bytes<const N: usize>(reader: &mut Cursor<&[u8]>) -> anyhow::Result<[u8; N]> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{DevicePropDataType, DevicePropDesc, DevicePropForm, DevicePropValue};

    #[test]
    fn descriptor_decodes_writable_scalar_without_form() {
        let bytes = [
            0x6b, 0xd3, // DevicePropertyCode
            0x02, 0x00, // DataType: UINT8
            0x01, // GetSet: writable
            100,  // FactoryDefaultValue
            80,   // CurrentValue
            0x00, // FormFlag: None
        ];

        let descriptor =
            DevicePropDesc::decode(&bytes).expect("valid UINT8 descriptor must decode");

        assert_eq!(descriptor.property_code, 0xd36b);
        assert_eq!(descriptor.data_type, DevicePropDataType::UInt8);
        assert!(descriptor.writable);
        assert_eq!(descriptor.factory_default, DevicePropValue::UInt(100));
        assert_eq!(descriptor.current, DevicePropValue::UInt(80));
        assert_eq!(descriptor.form, DevicePropForm::None);
    }

    #[test]
    fn descriptor_decodes_every_ptp_scalar_datatype() {
        let cases: [(u16, Vec<u8>, DevicePropValue); 10] = [
            (0x0001, vec![0xff], DevicePropValue::Int(-1)),
            (
                0x0003,
                (-2_i16).to_le_bytes().to_vec(),
                DevicePropValue::Int(-2),
            ),
            (
                0x0004,
                3_u16.to_le_bytes().to_vec(),
                DevicePropValue::UInt(3),
            ),
            (
                0x0005,
                (-4_i32).to_le_bytes().to_vec(),
                DevicePropValue::Int(-4),
            ),
            (
                0x0006,
                5_u32.to_le_bytes().to_vec(),
                DevicePropValue::UInt(5),
            ),
            (
                0x0007,
                (-6_i64).to_le_bytes().to_vec(),
                DevicePropValue::Int(-6),
            ),
            (
                0x0008,
                7_u64.to_le_bytes().to_vec(),
                DevicePropValue::UInt(7),
            ),
            (
                0x0009,
                (-8_i128).to_le_bytes().to_vec(),
                DevicePropValue::Int(-8),
            ),
            (
                0x000a,
                9_u128.to_le_bytes().to_vec(),
                DevicePropValue::UInt(9),
            ),
            (
                0xffff,
                vec![2, b'A', 0, 0, 0],
                DevicePropValue::String("A".to_owned()),
            ),
        ];

        for (datatype, encoded_value, expected) in cases {
            let mut bytes = Vec::from([0x01, 0xd0]);
            bytes.extend(datatype.to_le_bytes());
            bytes.push(0);
            bytes.extend(&encoded_value);
            bytes.extend(&encoded_value);
            bytes.push(0);

            let descriptor = DevicePropDesc::decode(&bytes)
                .expect("every standard PTP scalar datatype must decode");

            assert_eq!(descriptor.factory_default, expected);
        }
    }

    #[test]
    fn descriptor_decodes_ptp_array_datatype_for_vendor_blob_property() {
        let bytes = [
            0x85, 0xd1, // DevicePropertyCode
            0x02, 0x40, // DataType: AUINT8
            0x01, // GetSet: writable
            0x02, 0x00, 0x00, 0x00, 1, 2, // FactoryDefaultValue
            0x01, 0x00, 0x00, 0x00, 3,    // CurrentValue
            0x00, // FormFlag: None
        ];

        let descriptor = DevicePropDesc::decode(&bytes)
            .expect("valid PTP array property descriptor must decode");

        assert_eq!(descriptor.data_type.code(), 0x4002);
        assert_eq!(
            descriptor.current,
            DevicePropValue::Array(vec![DevicePropValue::UInt(3)])
        );
        assert!(
            descriptor
                .validate_serialized_candidate(&[2, 0, 0, 0, 4, 5])
                .is_ok()
        );
    }

    #[test]
    fn array_count_exceeding_payload_is_rejected_before_allocation() {
        let mut bytes = Vec::from([
            0x01, 0xd0, // DevicePropertyCode
            0x0a, 0x40, // DataType: AUINT128
            0x01, // GetSet: writable
        ]);
        // FactoryDefaultValue: declares the maximum permitted element count but
        // supplies no element bytes at all.
        bytes.extend((4 * 1024 * 1024_u32).to_le_bytes());

        let error = DevicePropDesc::decode(&bytes)
            .expect_err("array count exceeding its payload must be rejected before allocation");

        assert!(error.to_string().contains("larger than its payload"));
    }

    #[test]
    fn array_within_its_payload_but_over_the_allocation_budget_is_rejected() {
        // One more byte-typed element than the decoded-value budget allows.
        // The payload is fully present, so only the in-memory amplification
        // (32 bytes per decoded element) can reject it.
        let count = super::MAX_DEVICE_PROP_VALUES_ALLOCATION_BYTES
            / std::mem::size_of::<super::DevicePropValue>()
            + 1;
        let mut bytes = Vec::from([
            0x01, 0xd0, // DevicePropertyCode
            0x02, 0x40, // DataType: AUINT8
            0x01, // GetSet: writable
        ]);
        bytes.extend(u32::try_from(count).expect("count fits u32").to_le_bytes());
        bytes.resize(bytes.len() + count, 0xab);

        let error = DevicePropDesc::decode(&bytes)
            .expect_err("a payload-backed array must still respect the allocation budget");

        assert!(error.to_string().contains("allocation budget"), "{error}");
    }

    #[test]
    fn array_matching_its_payload_still_decodes() {
        let mut bytes = Vec::from([
            0x01, 0xd0, // DevicePropertyCode
            0x04, 0x40, // DataType: AUINT16
            0x01, // GetSet: writable
        ]);
        // FactoryDefaultValue: 2 elements, exactly 4 element bytes.
        bytes.extend(2_u32.to_le_bytes());
        bytes.extend(10_u16.to_le_bytes());
        bytes.extend(20_u16.to_le_bytes());
        // CurrentValue: 2 elements, exactly 4 element bytes.
        bytes.extend(2_u32.to_le_bytes());
        bytes.extend(30_u16.to_le_bytes());
        bytes.extend(40_u16.to_le_bytes());
        bytes.push(0x00); // FormFlag: None

        let descriptor = DevicePropDesc::decode(&bytes)
            .expect("array whose declared count matches its payload must decode");

        assert_eq!(
            descriptor.current,
            DevicePropValue::Array(vec![DevicePropValue::UInt(30), DevicePropValue::UInt(40)])
        );
    }

    #[test]
    fn enumeration_count_exceeding_payload_is_rejected_before_allocation() {
        let mut bytes = Vec::from([
            0x01, 0xd0, // DevicePropertyCode
            0x04, 0x00, // DataType: UINT16
            0x01, // GetSet: writable
            0x00, 0x00, // FactoryDefaultValue
            0x00, 0x00, // CurrentValue
            0x02, // FormFlag: Enumeration
        ]);
        // NumberOfValues: declares the maximum u16 count but supplies no
        // element bytes at all.
        bytes.extend(u16::MAX.to_le_bytes());

        let error = DevicePropDesc::decode(&bytes).expect_err(
            "enumeration count exceeding its payload must be rejected before allocation",
        );

        assert!(error.to_string().contains("larger than its payload"));
    }

    #[test]
    fn enumeration_matching_its_payload_still_decodes() {
        let bytes = [
            0x01, 0xd0, // DevicePropertyCode
            0x04, 0x00, // DataType: UINT16
            0x01, // GetSet: writable
            0x0a, 0x00, // FactoryDefaultValue
            0x14, 0x00, // CurrentValue
            0x02, // FormFlag: Enumeration
            0x02, 0x00, // NumberOfValues
            0x0a, 0x00, // SupportedValue[0]
            0x14, 0x00, // SupportedValue[1]
        ];

        let descriptor = DevicePropDesc::decode(&bytes)
            .expect("enumeration whose declared count matches its payload must decode");

        assert_eq!(
            descriptor.form,
            DevicePropForm::Enumeration(
                vec![DevicePropValue::UInt(10), DevicePropValue::UInt(20),]
            )
        );
    }

    #[test]
    fn descriptor_decodes_range_form_using_declared_datatype() {
        let bytes = [
            0x01, 0xd0, // DevicePropertyCode
            0x04, 0x00, // DataType: UINT16
            0x01, // GetSet: writable
            0x00, 0x00, // FactoryDefaultValue
            0x04, 0x00, // CurrentValue
            0x01, // FormFlag: Range
            0x00, 0x00, // MinimumValue
            0x0a, 0x00, // MaximumValue
            0x02, 0x00, // StepSize
        ];

        let descriptor = DevicePropDesc::decode(&bytes).expect("valid range form must decode");

        assert_eq!(
            descriptor.form,
            DevicePropForm::Range {
                minimum: DevicePropValue::UInt(0),
                maximum: DevicePropValue::UInt(10),
                step: DevicePropValue::UInt(2),
            }
        );
    }

    #[test]
    fn descriptor_decodes_enumeration_form_using_declared_datatype() {
        let bytes = [
            0x01, 0xd0, // DevicePropertyCode
            0x03, 0x00, // DataType: INT16
            0x01, // GetSet: writable
            0x00, 0x00, // FactoryDefaultValue
            0x01, 0x00, // CurrentValue
            0x02, // FormFlag: Enumeration
            0x03, 0x00, // NumberOfValues
            0xff, 0xff, // SupportedValue[0]
            0x00, 0x00, // SupportedValue[1]
            0x01, 0x00, // SupportedValue[2]
        ];

        let descriptor =
            DevicePropDesc::decode(&bytes).expect("valid enumeration form must decode");

        assert_eq!(
            descriptor.form,
            DevicePropForm::Enumeration(vec![
                DevicePropValue::Int(-1),
                DevicePropValue::Int(0),
                DevicePropValue::Int(1),
            ])
        );
    }

    #[test]
    fn descriptor_decodes_a_string_enumeration_form() {
        // PTP lets a string property enumerate its permitted values; the
        // battery property is a string among the required properties, so a
        // camera that answers with one must not fail the decode.
        let bytes = [
            0x6b, 0xd3, // DevicePropertyCode
            0xff, 0xff, // DataType: STR
            0x00, // GetSet: read-only
            0x02, b'A', 0x00, 0x00, 0x00, // FactoryDefaultValue: "A"
            0x02, b'B', 0x00, 0x00, 0x00, // CurrentValue: "B"
            0x02, // FormFlag: Enumeration
            0x02, 0x00, // NumberOfValues
            0x02, b'A', 0x00, 0x00, 0x00, // SupportedValue[0]
            0x02, b'B', 0x00, 0x00, 0x00, // SupportedValue[1]
        ];

        let descriptor =
            DevicePropDesc::decode(&bytes).expect("a string enumeration form must decode");

        assert_eq!(
            descriptor.form,
            DevicePropForm::Enumeration(vec![
                DevicePropValue::String("A".to_owned()),
                DevicePropValue::String("B".to_owned()),
            ])
        );
        assert_eq!(descriptor.current, DevicePropValue::String("B".to_owned()));
    }

    #[test]
    fn a_string_enumeration_longer_than_its_payload_is_rejected() {
        // Each string costs at least one byte, so the declared count is
        // bounded by the payload before any allocation.
        let bytes = [
            0x6b, 0xd3, // DevicePropertyCode
            0xff, 0xff, // DataType: STR
            0x00, // GetSet: read-only
            0x00, // FactoryDefaultValue: ""
            0x00, // CurrentValue: ""
            0x02, // FormFlag: Enumeration
            0xff, 0xff, // NumberOfValues: 65535
            0x00, // SupportedValue[0]
        ];

        let error = DevicePropDesc::decode(&bytes)
            .expect_err("a string enumeration must not outrun its payload");

        assert!(
            error.to_string().contains("larger than its payload"),
            "{error}"
        );
    }

    #[test]
    fn descriptor_rejects_current_value_outside_declared_enumeration() {
        let bytes = [
            0x01, 0xd0, // DevicePropertyCode
            0x04, 0x00, // DataType: UINT16
            0x01, // GetSet: writable
            0x01, 0x00, // FactoryDefaultValue
            0x02, 0x00, // CurrentValue
            0x02, // FormFlag: Enumeration
            0x01, 0x00, // NumberOfValues
            0x01, 0x00, // SupportedValue[0]
        ];

        let error = DevicePropDesc::decode(&bytes)
            .expect_err("descriptor current value must belong to its enumeration");

        assert!(error.to_string().contains("current value"));
    }

    #[test]
    fn descriptor_rejects_invalid_range_step() {
        let bytes = [
            0x01, 0xd0, // DevicePropertyCode
            0x04, 0x00, // DataType: UINT16
            0x01, // GetSet: writable
            0x00, 0x00, // FactoryDefaultValue
            0x00, 0x00, // CurrentValue
            0x01, // FormFlag: Range
            0x00, 0x00, // MinimumValue
            0x0a, 0x00, // MaximumValue
            0x00, 0x00, // StepSize
        ];

        let error = DevicePropDesc::decode(&bytes).expect_err("zero range step must fail closed");

        assert!(error.to_string().contains("declared range"));
    }

    #[test]
    fn descriptor_rejects_candidate_when_property_is_read_only() {
        let descriptor = DevicePropDesc {
            property_code: 0xd001,
            data_type: DevicePropDataType::UInt8,
            writable: false,
            factory_default: DevicePropValue::UInt(0),
            current: DevicePropValue::UInt(0),
            form: DevicePropForm::None,
        };

        let error = descriptor
            .validate_serialized_candidate(&[1])
            .expect_err("read-only property must reject every candidate");

        assert!(error.to_string().contains("read-only"));
    }

    #[test]
    fn descriptor_rejects_candidate_that_does_not_exactly_match_datatype() {
        let descriptor = DevicePropDesc {
            property_code: 0xd001,
            data_type: DevicePropDataType::UInt16,
            writable: true,
            factory_default: DevicePropValue::UInt(0),
            current: DevicePropValue::UInt(0),
            form: DevicePropForm::None,
        };

        let short = descriptor
            .validate_serialized_candidate(&[1])
            .expect_err("truncated UINT16 candidate must be rejected");
        let trailing = descriptor
            .validate_serialized_candidate(&[1, 0, 0])
            .expect_err("UINT16 candidate with trailing data must be rejected");

        assert!(short.to_string().contains("candidate"));
        assert!(trailing.to_string().contains("trailing"));
    }

    #[test]
    fn descriptor_enforces_numeric_range_and_step_for_candidate() {
        let descriptor = DevicePropDesc {
            property_code: 0xd001,
            data_type: DevicePropDataType::UInt16,
            writable: true,
            factory_default: DevicePropValue::UInt(2),
            current: DevicePropValue::UInt(2),
            form: DevicePropForm::Range {
                minimum: DevicePropValue::UInt(2),
                maximum: DevicePropValue::UInt(10),
                step: DevicePropValue::UInt(2),
            },
        };

        descriptor
            .validate_serialized_candidate(&8_u16.to_le_bytes())
            .expect("aligned candidate within range must be accepted");
        let misaligned = descriptor
            .validate_serialized_candidate(&9_u16.to_le_bytes())
            .expect_err("misaligned candidate must be rejected");
        let outside = descriptor
            .validate_serialized_candidate(&12_u16.to_le_bytes())
            .expect_err("candidate above maximum must be rejected");

        assert!(misaligned.to_string().contains("range"));
        assert!(outside.to_string().contains("range"));
    }

    #[test]
    fn descriptor_enforces_enumerated_values_for_candidate() {
        let descriptor = DevicePropDesc {
            property_code: 0xd001,
            data_type: DevicePropDataType::Int16,
            writable: true,
            factory_default: DevicePropValue::Int(0),
            current: DevicePropValue::Int(0),
            form: DevicePropForm::Enumeration(vec![
                DevicePropValue::Int(-1),
                DevicePropValue::Int(0),
                DevicePropValue::Int(1),
            ]),
        };

        descriptor
            .validate_serialized_candidate(&(-1_i16).to_le_bytes())
            .expect("listed candidate must be accepted");
        let error = descriptor
            .validate_serialized_candidate(&2_i16.to_le_bytes())
            .expect_err("unlisted candidate must be rejected");

        assert!(error.to_string().contains("enumeration"));
    }

    #[test]
    fn datatype_reports_its_ptp_wire_code() {
        let cases = [
            (DevicePropDataType::Int8, 0x0001),
            (DevicePropDataType::UInt8, 0x0002),
            (DevicePropDataType::Int16, 0x0003),
            (DevicePropDataType::UInt16, 0x0004),
            (DevicePropDataType::Int32, 0x0005),
            (DevicePropDataType::UInt32, 0x0006),
            (DevicePropDataType::Int64, 0x0007),
            (DevicePropDataType::UInt64, 0x0008),
            (DevicePropDataType::Int128, 0x0009),
            (DevicePropDataType::UInt128, 0x000a),
            (DevicePropDataType::String, 0xffff),
        ];

        for (datatype, expected_code) in cases {
            assert_eq!(datatype.code(), expected_code);
        }
    }
}
