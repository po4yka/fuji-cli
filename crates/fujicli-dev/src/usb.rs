use std::{fmt, str::FromStr};

use anyhow::{anyhow, bail};

#[derive(Clone, Copy, Debug)]
pub struct Location {
    pub bus: u8,
    pub address: u8,
}

impl FromStr for Location {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (bus, address) = value
            .split_once('.')
            .ok_or_else(|| anyhow!("invalid device format: {value}; expected <BUS>.<ADDRESS>"))?;
        Ok(Self {
            bus: bus
                .parse()
                .map_err(|_| anyhow!("invalid USB bus number: {bus}"))?,
            address: address
                .parse()
                .map_err(|_| anyhow!("invalid USB address: {address}"))?,
        })
    }
}

impl fmt::Display for Location {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.bus, self.address)
    }
}

pub fn exact_device(location: Location) -> anyhow::Result<rusb::Device<rusb::GlobalContext>> {
    for device in rusb::devices()?.iter() {
        if device.bus_number() == location.bus && device.address() == location.address {
            return Ok(device);
        }
    }
    bail!("no USB device found at exact location {location}")
}

#[cfg(test)]
mod tests {
    use super::Location;

    #[test]
    fn parses_bus_and_address() {
        let location = "3.42".parse::<Location>().expect("valid location");
        assert_eq!(location.bus, 3);
        assert_eq!(location.address, 42);
    }

    #[test]
    fn parses_zero_bus_and_address() {
        let location = "0.0".parse::<Location>().expect("valid location");
        assert_eq!(location.bus, 0);
        assert_eq!(location.address, 0);
    }

    #[test]
    fn rejects_missing_separator() {
        let error = "3".parse::<Location>().expect_err("missing separator");
        assert!(
            error.to_string().contains("invalid device format"),
            "got: {error}"
        );
    }

    #[test]
    fn rejects_non_numeric_bus() {
        let error = "a.1".parse::<Location>().expect_err("non-numeric bus");
        assert!(
            error.to_string().contains("invalid USB bus number"),
            "got: {error}"
        );
    }

    #[test]
    fn rejects_non_numeric_address() {
        let error = "1.b".parse::<Location>().expect_err("non-numeric address");
        assert!(
            error.to_string().contains("invalid USB address"),
            "got: {error}"
        );
    }

    #[test]
    fn rejects_out_of_range_components() {
        assert!("300.1".parse::<Location>().is_err());
        assert!("1.999".parse::<Location>().is_err());
    }

    #[test]
    fn display_round_trips_the_parsed_form() {
        let location = "3.42".parse::<Location>().expect("valid location");
        assert_eq!(format!("{location}"), "3.42");
    }
}
