//! Append-only JSONL audit log for the `dangerous-reverse-engineering` probe
//! surface. The field set is a contract (`docs/contributors/reversing.md`,
//! "Requirements for Any Future Dangerous Probe"): keep it stable once
//! first written, and never add raw serials, argv, full paths, property or
//! backup payloads, custom setting names, arbitrary camera strings, or full
//! error chains.

use std::{fs::OpenOptions, io::Write as _, path::Path};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;

use anyhow::Context as _;
use serde_json::json;

/// One durable audit record. Every field here is allowed by the contract in
/// `reversing.md`; do not widen this struct without updating that contract.
#[derive(Debug, Clone)]
pub struct AuditRecord {
    pub timestamp: String,
    pub tool_version: String,
    pub invocation_id: String,
    pub operation: String,
    pub risk_class: String,
    pub ptp_operation_codes: Vec<String>,
    pub usb_location: String,
    pub vid_pid: String,
    pub model: String,
    pub firmware: String,
    pub serial_fingerprint: String,
    pub pre_backup_sha256: String,
    pub outcome: String,
}

impl AuditRecord {
    /// Builds the exact allowlisted JSON object for this record. Kept
    /// separate from `serde_json::Value` construction call sites so the
    /// allowlist stays in exactly one place.
    fn to_json(&self) -> serde_json::Value {
        json!({
            "timestamp": self.timestamp,
            "tool_version": self.tool_version,
            "invocation_id": self.invocation_id,
            "operation": self.operation,
            "risk_class": self.risk_class,
            "ptp_operation_codes": self.ptp_operation_codes,
            "usb_location": self.usb_location,
            "vid_pid": self.vid_pid,
            "model": self.model,
            "firmware": self.firmware,
            "serial_fingerprint": self.serial_fingerprint,
            "pre_backup_sha256": self.pre_backup_sha256,
            "outcome": self.outcome,
        })
    }
}

/// Appends one JSONL line to `path`, creating it with restrictive
/// permissions (Unix mode `0600`) if it does not already exist. Never
/// truncates or overwrites existing lines.
pub fn append(path: &Path, record: &AuditRecord) -> anyhow::Result<()> {
    let mut line = serde_json::to_vec(&record.to_json()).context("serializing audit record")?;
    line.push(b'\n');

    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options
        .open(path)
        .with_context(|| format!("opening audit log {}", path.display()))?;
    file.write_all(&line).context("appending audit record")?;
    file.flush().context("flushing audit log")?;
    file.sync_all().context("syncing audit log")?;
    Ok(())
}

/// Truncates `value` to at most `max_bytes` bytes on a UTF-8 char boundary,
/// bounding camera-reported strings (model/firmware) before they enter the
/// audit log.
pub fn bound(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{AuditRecord, bound};

    fn sample_record() -> AuditRecord {
        AuditRecord {
            timestamp: "1735603200".to_owned(),
            tool_version: "0.2.0".to_owned(),
            invocation_id: "12345-67890".to_owned(),
            operation: "probe_simulation_namespace".to_owned(),
            risk_class: "state_changing".to_owned(),
            ptp_operation_codes: vec!["0x1015".to_owned(), "0x1016".to_owned()],
            usb_location: "1.2".to_owned(),
            vid_pid: "04cb:02cb".to_owned(),
            model: "X-T5".to_owned(),
            firmware: "4.31".to_owned(),
            serial_fingerprint: "a".repeat(64),
            pre_backup_sha256: "b".repeat(64),
            outcome: "attempted".to_owned(),
        }
    }

    fn allowlisted_keys() -> BTreeSet<&'static str> {
        [
            "timestamp",
            "tool_version",
            "invocation_id",
            "operation",
            "risk_class",
            "ptp_operation_codes",
            "usb_location",
            "vid_pid",
            "model",
            "firmware",
            "serial_fingerprint",
            "pre_backup_sha256",
            "outcome",
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn audit_record_serializes_only_the_allowlisted_fields() {
        let record = sample_record();
        let value = record.to_json();
        let object = value
            .as_object()
            .expect("audit record must be a JSON object");

        let actual: BTreeSet<&str> = object.keys().map(String::as_str).collect();

        assert_eq!(actual, allowlisted_keys());
    }

    /// A terminal record (same fields as the pre-write `attempted` record,
    /// only `outcome` differs) must serialize to the exact same allowlist --
    /// the contract is identical for both lines of one attempt.
    #[test]
    fn terminal_outcome_record_serializes_only_the_allowlisted_fields() {
        let mut record = sample_record();
        record.outcome = "restore_failed".to_owned();
        let value = record.to_json();
        let object = value
            .as_object()
            .expect("terminal audit record must be a JSON object");

        let actual: BTreeSet<&str> = object.keys().map(String::as_str).collect();

        assert_eq!(actual, allowlisted_keys());
    }

    #[test]
    fn audit_record_never_contains_forbidden_markers() {
        let record = sample_record();
        let serialized = serde_json::to_string(&record.to_json())
            .expect("audit record must serialize to a string");

        for forbidden in [
            "argv",
            "raw_serial",
            "backup_payload",
            "property_payload",
            "error_chain",
            "custom_setting_name",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "audit record must not contain {forbidden}"
            );
        }
    }

    #[test]
    fn bound_leaves_short_strings_untouched() {
        assert_eq!(bound("X-T5", 64), "X-T5");
    }

    #[test]
    fn bound_truncates_long_strings_on_a_char_boundary() {
        let long = "x".repeat(200);
        let truncated = bound(&long, 64);
        assert_eq!(truncated.len(), 64);
    }
}
