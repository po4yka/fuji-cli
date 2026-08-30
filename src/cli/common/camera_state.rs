use std::fmt;

/// Marker attached to an error chain when a state-changing camera operation
/// has already been sent to the camera and its outcome could not be
/// confirmed afterward. Callers attach it alongside the existing
/// human-readable `.context(...)` explanation; `main` downcasts for this
/// type to choose a distinct, non-retryable exit code.
///
/// This type carries no data and adds no prose of its own -- the
/// descriptive message for the operator comes entirely from the surrounding
/// context chain.
#[derive(Debug)]
pub struct CameraStateUnknown;

impl fmt::Display for CameraStateUnknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "camera state unknown")
    }
}

impl std::error::Error for CameraStateUnknown {}
