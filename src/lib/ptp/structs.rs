use binrw::{BinRead, BinWrite};
use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(Debug, BinRead, BinWrite)]
#[brw(little)]
pub struct DeviceInfo {
    pub version: u16,
    pub vendor_ex_id: u32,
    pub vendor_ex_version: u16,
    #[br(parse_with = crate::ptp::codec::read_ptp_string)]
    #[bw(write_with = crate::ptp::codec::write_ptp_string)]
    pub vendor_extension_desc: String,
    pub functional_mode: u16,
    #[br(parse_with = crate::ptp::codec::read_ptp_array)]
    #[bw(write_with = crate::ptp::codec::write_ptp_array)]
    pub operations_supported: Vec<u16>,
    #[br(parse_with = crate::ptp::codec::read_ptp_array)]
    #[bw(write_with = crate::ptp::codec::write_ptp_array)]
    pub events_supported: Vec<u16>,
    #[br(parse_with = crate::ptp::codec::read_ptp_array)]
    #[bw(write_with = crate::ptp::codec::write_ptp_array)]
    pub device_properties_supported: Vec<u16>,
    #[br(parse_with = crate::ptp::codec::read_ptp_array)]
    #[bw(write_with = crate::ptp::codec::write_ptp_array)]
    pub capture_formats: Vec<u16>,
    #[br(parse_with = crate::ptp::codec::read_ptp_array)]
    #[bw(write_with = crate::ptp::codec::write_ptp_array)]
    pub image_formats: Vec<u16>,
    #[br(parse_with = crate::ptp::codec::read_ptp_string)]
    #[bw(write_with = crate::ptp::codec::write_ptp_string)]
    pub manufacturer: String,
    #[br(parse_with = crate::ptp::codec::read_ptp_string)]
    #[bw(write_with = crate::ptp::codec::write_ptp_string)]
    pub model: String,
    #[br(parse_with = crate::ptp::codec::read_ptp_string)]
    #[bw(write_with = crate::ptp::codec::write_ptp_string)]
    pub device_version: String,
    #[br(parse_with = crate::ptp::codec::read_ptp_string)]
    #[bw(write_with = crate::ptp::codec::write_ptp_string)]
    pub serial_number: String,
}

#[cfg(test)]
mod tests {
    use crate::ptp::codec::{decode_exact, encode};

    use super::{DeviceInfo, ObjectFormat, ObjectInfo};

    #[test]
    fn binrw_device_info_encoding_preserves_ptp_field_order() {
        let value = DeviceInfo {
            version: 100,
            vendor_ex_id: 0x01020304,
            vendor_ex_version: 200,
            vendor_extension_desc: "Fujifilm".to_owned(),
            functional_mode: 1,
            operations_supported: vec![0x1001, 0x1002],
            events_supported: vec![0x4002],
            device_properties_supported: vec![0x5001],
            capture_formats: vec![0x3801],
            image_formats: vec![0x3801, 0x3808],
            manufacturer: "FUJIFILM".to_owned(),
            model: "X-T5".to_owned(),
            device_version: "1.00".to_owned(),
            serial_number: "12345".to_owned(),
        };

        let encoded = encode(&value).expect("binrw DeviceInfo encoding must succeed");

        assert_eq!(&encoded[..11], [100, 0, 4, 3, 2, 1, 200, 0, 9, b'F', 0]);
    }

    #[test]
    fn binrw_device_info_round_trips_representative_fields() {
        let value = DeviceInfo {
            version: 100,
            vendor_ex_id: 0x01020304,
            vendor_ex_version: 200,
            vendor_extension_desc: "Fujifilm".to_owned(),
            functional_mode: 1,
            operations_supported: vec![0x1001, 0x1002],
            events_supported: vec![0x4002],
            device_properties_supported: vec![0x5001],
            capture_formats: vec![0x3801],
            image_formats: vec![0x3801, 0x3808],
            manufacturer: "FUJIFILM".to_owned(),
            model: "X-T5".to_owned(),
            device_version: "1.00".to_owned(),
            serial_number: "12345".to_owned(),
        };
        let bytes = encode(&value).expect("binrw DeviceInfo encoding must succeed");

        let decoded =
            decode_exact::<DeviceInfo>(&bytes).expect("binrw DeviceInfo decoding must succeed");

        assert_eq!(decoded.version, value.version);
        assert_eq!(decoded.operations_supported, value.operations_supported);
        assert_eq!(decoded.vendor_extension_desc, value.vendor_extension_desc);
        assert_eq!(decoded.model, value.model);
    }

    #[test]
    fn binrw_object_info_encoding_preserves_ptp_field_order() {
        let value = ObjectInfo {
            storage_id: 0x01020304,
            object_format: ObjectFormat::FujiRAF,
            protection_status: 1,
            compressed_size: 0x10203040,
            thumb_format: 0x3801,
            thumb_compressed_size: 1024,
            thumb_width: 160,
            thumb_height: 120,
            image_width: 6240,
            image_height: 4160,
            image_bit_depth: 14,
            parent_object: 7,
            association_type: 1,
            association_desc: 2,
            sequence_number: 3,
            filename: "DSCF0001.RAF".to_owned(),
            date_created: "20260829T120000".to_owned(),
            date_modified: "20260829T120100".to_owned(),
            keywords: "raw".to_owned(),
        };

        let encoded = encode(&value).expect("binrw ObjectInfo encoding must succeed");

        assert_eq!(&encoded[..12], [4, 3, 2, 1, 2, 248, 1, 0, 64, 48, 32, 16]);
    }

    #[test]
    fn binrw_object_info_round_trips_representative_fields() {
        let value = ObjectInfo {
            storage_id: 0x01020304,
            object_format: ObjectFormat::FujiRAF,
            protection_status: 1,
            compressed_size: 0x10203040,
            thumb_format: 0x3801,
            thumb_compressed_size: 1024,
            thumb_width: 160,
            thumb_height: 120,
            image_width: 6240,
            image_height: 4160,
            image_bit_depth: 14,
            parent_object: 7,
            association_type: 1,
            association_desc: 2,
            sequence_number: 3,
            filename: "DSCF0001.RAF".to_owned(),
            date_created: "20260829T120000".to_owned(),
            date_modified: "20260829T120100".to_owned(),
            keywords: "raw".to_owned(),
        };
        let bytes = encode(&value).expect("binrw ObjectInfo encoding must succeed");

        let decoded =
            decode_exact::<ObjectInfo>(&bytes).expect("binrw ObjectInfo decoding must succeed");

        assert_eq!(decoded.object_format, value.object_format);
        assert_eq!(decoded.compressed_size, value.compressed_size);
        assert_eq!(decoded.date_created, value.date_created);
        assert_eq!(decoded.filename, value.filename);
    }

    #[test]
    fn object_info_accepts_standard_exif_jpeg_format() {
        let mut bytes =
            encode(&ObjectInfo::default()).expect("default ObjectInfo encoding must succeed");
        bytes[4..6].copy_from_slice(&0x3801_u16.to_le_bytes());

        let decoded = decode_exact::<ObjectInfo>(&bytes)
            .expect("standard EXIF/JPEG ObjectInfo format must be supported");

        assert_eq!(u16::from(decoded.object_format), 0x3801);
    }
}

#[repr(u16)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive, Default, BinRead, BinWrite,
)]
#[brw(repr(u16))]
pub enum ObjectFormat {
    #[default]
    None = 0x0,
    ExifJpeg = 0x3801,
    FujiBackup = 0x5000,
    FujiRAF = 0xf802,
}

#[derive(Debug, Clone, Default, BinRead, BinWrite)]
#[brw(little)]
pub struct ObjectInfo {
    pub storage_id: u32,
    pub object_format: ObjectFormat,
    pub protection_status: u16,
    pub compressed_size: u32,
    pub thumb_format: u16,
    pub thumb_compressed_size: u32,
    pub thumb_width: u32,
    pub thumb_height: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub image_bit_depth: u32,
    pub parent_object: u32,
    pub association_type: u16,
    pub association_desc: u32,
    pub sequence_number: u32,
    #[br(parse_with = crate::ptp::codec::read_ptp_string)]
    #[bw(write_with = crate::ptp::codec::write_ptp_string)]
    pub filename: String,
    #[br(parse_with = crate::ptp::codec::read_ptp_string)]
    #[bw(write_with = crate::ptp::codec::write_ptp_string)]
    pub date_created: String,
    #[br(parse_with = crate::ptp::codec::read_ptp_string)]
    #[bw(write_with = crate::ptp::codec::write_ptp_string)]
    pub date_modified: String,
    #[br(parse_with = crate::ptp::codec::read_ptp_string)]
    #[bw(write_with = crate::ptp::codec::write_ptp_string)]
    pub keywords: String,
}
