# User Guide

`fujicli` exposes device, backup, film-simulation, and in-camera RAW workflows.
Runtime preflight, not command visibility, decides whether a camera operation is
authorized. Consult the [support matrix](support.md) for current device status.

Start with the [first safe session](getting-started/first-safe-session.md).
It verifies the executable without hardware, discovers USB devices without a
PTP session, and routes access failures before any state-changing workflow.

## Choose a Task

| Goal | Guide |
| --- | --- |
| Install the CLI | [Installation](installation.md) |
| Verify the CLI and discover a camera safely | [First safe session](getting-started/first-safe-session.md) |
| Export and inspect a camera backup | [Export and inspect a backup](how-to/export-and-inspect-backup.md) |
| Validate and restore a backup | [Dry-run and restore a backup](how-to/dry-run-and-restore-backup.md) |
| Fix USB permissions, drivers, or a busy camera | [Troubleshoot device access](how-to/troubleshoot-device-access.md) |
| Check command syntax and constraints | [CLI reference](reference/cli.md) |
| Integrate stdout, stderr, JSON, or files | [Output and JSON](reference/output-and-json.md) |
| Handle process results safely | [Exit codes](reference/exit-codes.md) |
| Understand why commands fail closed | [Fail-closed safety model](explanation/fail-closed-safety-model.md) |
| Understand acceptable hardware evidence | [Physical-evidence model](explanation/physical-evidence-model.md) |

## Current Boundary

Command grammar can exist for a capability that is not authorized on a
connected camera. Unknown firmware, USB mode, capability profile, or wire
descriptor fails closed. Normal commands have no experimental override.

Never retry a state-changing command automatically after an unknown-state
result. Keep camera serials, backup artifacts, RAF/JPEG files, custom-setting
names, and unreviewed verbose logs out of public reports.
