use std::{fmt, io};

use crate::ptp::ResponseCode;

#[derive(Debug)]
pub enum Error {
    Response(u16),
    Malformed(String),
    Usb(rusb::Error),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Response(r) => {
                let name = ResponseCode::try_from(r)
                    .map_or_else(|_| "Unknown".to_string(), |c| format!("{c:?}"));
                write!(f, "{name} (0x{r:04x})")
            }
            Self::Usb(ref e) => write!(f, "USB error: {e}"),
            Self::Io(ref e) => write!(f, "IO error: {e}"),
            Self::Malformed(ref e) => write!(f, "{e}"),
        }
    }
}

impl ::std::error::Error for Error {
    fn cause(&self) -> Option<&dyn ::std::error::Error> {
        match *self {
            Self::Usb(ref e) => Some(e),
            Self::Io(ref e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusb::Error> for Error {
    fn from(e: rusb::Error) -> Self {
        Self::Usb(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::UnexpectedEof => {
                Self::Malformed("Unexpected end of message".to_string())
            }
            _ => Self::Io(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn known_response_code_prints_debug_name_and_lowercase_hex() {
        assert_eq!(Error::Response(0x2001).to_string(), "Ok (0x2001)");
        assert_eq!(Error::Response(0x2019).to_string(), "DeviceBusy (0x2019)");
    }

    #[test]
    fn unknown_response_code_prints_unknown() {
        assert_eq!(Error::Response(0xffff).to_string(), "Unknown (0xffff)");
    }

    #[test]
    fn usb_error_display_keeps_the_source_text() {
        // The tail belongs to rusb, so only the prefix is pinned here.
        let rendered = Error::from(rusb::Error::Access).to_string();
        assert!(rendered.starts_with("USB error: "), "got: {rendered}");
    }

    #[test]
    fn io_error_display_keeps_the_source_text() {
        let rendered = Error::Io(std::io::Error::other("boom")).to_string();
        assert!(rendered.starts_with("IO error: "), "got: {rendered}");
        // Malformed passes its message through unchanged.
        assert_eq!(
            Error::Malformed("bad shape".to_owned()).to_string(),
            "bad shape"
        );
    }

    #[test]
    fn unexpected_eof_maps_to_malformed_end_of_message() {
        let error = Error::from(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        assert!(
            matches!(&error, Error::Malformed(message) if message == "Unexpected end of message"),
            "got: {error:?}"
        );
    }

    #[test]
    fn other_io_kinds_stay_io_variant() {
        let error = Error::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(matches!(error, Error::Io(_)));
    }
}
