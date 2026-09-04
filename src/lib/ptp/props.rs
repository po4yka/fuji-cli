use anyhow::Context;
use num_enum::{IntoPrimitive, TryFromPrimitive};

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum DevicePropCode {
    FujiRawConversionRun = 0xD183,
    FujiRawConversionProfile = 0xD185,
    FujiBatteryInfo2 = 0xD36B,
}

/// Parses the percentage field out of a Fuji `0xD36B` battery info string
/// (for example `"100,0,0"`, where the first comma-separated field is the
/// battery percentage). Shared by every reader of `FujiBatteryInfo2` so the
/// strictness of the parse cannot drift between call sites.
pub(crate) fn parse_fuji_battery_percent(battery: &str) -> anyhow::Result<u8> {
    let percent: u8 = battery
        .split(',')
        .next()
        .ok_or_else(|| anyhow::anyhow!("camera battery response is empty"))?
        .trim()
        .parse()
        .with_context(|| format!("failed to parse camera battery percentage from {battery:?}"))?;
    anyhow::ensure!(
        percent <= 100,
        "camera battery percentage {percent} exceeds 100"
    );
    Ok(percent)
}

#[cfg(test)]
mod tests {
    use super::parse_fuji_battery_percent;

    #[test]
    fn parses_percentage_from_full_battery_string() {
        assert_eq!(parse_fuji_battery_percent("100,0,0").unwrap(), 100);
        assert_eq!(parse_fuji_battery_percent("65,0,0").unwrap(), 65);
    }

    #[test]
    fn rejects_empty_battery_string() {
        assert!(parse_fuji_battery_percent("").is_err());
    }

    #[test]
    fn rejects_percentage_above_100() {
        let error = parse_fuji_battery_percent("101,0,0").unwrap_err();
        assert!(error.to_string().contains("exceeds 100"));
    }

    #[test]
    fn rejects_non_numeric_percentage() {
        assert!(parse_fuji_battery_percent("full,0,0").is_err());
    }
}
