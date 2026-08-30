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
