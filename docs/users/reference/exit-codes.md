# Exit Codes

Exit status is a public contract for scripts and wrappers. Interpret it
together with stdout, stderr, JSON shape, and the physical camera state.

| Status | Meaning | Automation guidance |
| --- | --- | --- |
| `0` | Success | Continue. For a mutation, success means the PTP operation completed under its documented contract; it may not prove persistence after reboot. |
| `1` | Failure before a camera write, or another safely retryable failure | Investigate the cause before retrying. Examples include no camera, a missing file, a non-clap argument error, or preflight rejection. |
| `2` | clap rejected command grammar | Correct the invalid subcommand, missing argument, conflict, or meaningless option combination. |
| `3` | A state-changing operation was sent but its outcome could not be confirmed | Do not retry automatically. Inspect the physical camera and establish its actual state first. |
| `130` | Ctrl-C before any camera write was sent | The process was interrupted before a camera mutation was dispatched, so nothing on the camera changed. |

Ctrl-C never abandons a PTP transfer mid-stream. An aborted bulk transfer can
leave the camera's USB pipe unusable until the cable is physically
reconnected, so the first Ctrl-C during any PTP transaction is latched until
that transaction returns.

Outside a camera write, the command then stops before the next transaction,
closes the session normally, and exits `130`. `simulation list`, `get`, and
`export` switch the camera's custom-setting slot while they read; an interrupt
during them is held until the original slot is restored and verified, and the
command then exits `130`.

During a camera write, the whole write runs to completion. The command then
exits `3` and prints do-not-retry guidance.

Once a camera write has been sent, every later Ctrl-C in the same process
exits `3`, including one that lands while `backup import` waits for the camera
to reconnect or exports the verification backup. The write already happened;
only its persistence is unconfirmed.

A second Ctrl-C forces immediate exit: `3` once a camera write is in progress
or was already sent, because the camera state is necessarily unknown,
otherwise `130`. After a forced quit during a transfer, disconnect and
reconnect the camera before the next command.
