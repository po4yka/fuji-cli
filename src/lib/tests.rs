use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use super::best_effort_close_session;

#[test]
fn drop_close_uses_a_short_absolute_deadline() {
    let start = Instant::now();
    let observed_deadline = RefCell::new(None);

    best_effort_close_session(
        true,
        true,
        || start,
        |deadline| {
            observed_deadline.replace(Some(deadline));
            Ok(())
        },
    );

    assert_eq!(
        observed_deadline.into_inner(),
        Some(start + Duration::from_secs(1))
    );
}

#[test]
fn explicitly_closed_camera_is_not_closed_again_by_drop() {
    let close_called = RefCell::new(false);

    best_effort_close_session(false, true, Instant::now, |_| {
        close_called.replace(true);
        Ok(())
    });

    assert!(!close_called.into_inner());
}
