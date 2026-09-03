# Exit Codes

Exit status is a public contract for scripts and wrappers. Interpret it
together with stdout, stderr, JSON shape, and the physical camera state.

| Status | Meaning | Automation guidance |
| --- | --- | --- |
| `0` | Success | Continue. For a mutation, success means the PTP operation completed under its documented contract; it may not prove persistence after reboot. |
| `1` | Failure before a camera write, or another safely retryable failure | Investigate the cause before retrying. Examples include no camera, a missing file, a non-clap argument error, or preflight rejection. |
| `2` | clap rejected command grammar | Correct the invalid subcommand, missing argument, conflict, or meaningless option combination. |
| `3` | A state-changing operation was sent but its outcome could not be confirmed | Do not retry automatically. Inspect the physical camera and establish its actual state first. |
| `130` | Ctrl-C outside a camera write | The process was interrupted before an in-flight camera mutation required unknown-state handling. |

Ctrl-C never abandons a PTP transfer mid-stream. An aborted bulk transfer can
leave the camera's USB pipe unusable until the cable is physically
reconnected, so the first Ctrl-C during any PTP transaction is latched until
that transaction returns.

Outside a camera write, the command then stops before the next transaction,
closes the session normally, and exits `130`.

During a camera write, the whole write runs to completion. The command then
exits `3` and prints do-not-retry guidance.

A second Ctrl-C forces immediate exit: `3` during a camera write because the
camera state is necessarily unknown, otherwise `130`. After a forced quit
during a transfer, disconnect and reconnect the camera before the next command.
