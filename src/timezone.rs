use std::sync::OnceLock;

use jiff::tz::TimeZone;

/// Hold the system timezone to prevent querying it each time it is needed.
pub(crate) static SYSTEM_TZ: OnceLock<TimeZone> = OnceLock::new();

/// Initializes the system time zone. Must be called only once.
///
/// # Errors
///
/// Returns an error is something goes wrong querying the system time zone.
///
/// # Panics
///
/// Panics if called more than once.
pub(crate) fn init_system_timezone() -> Result<(), jiff::Error> {
    let tz = TimeZone::try_system()?;

    SYSTEM_TZ
        .set(tz)
        .expect("init_system_timezone() should not be called more than once");

    Ok(())
}

/// Get the cached system time zone.
///
/// # Panics
///
/// Panics if [`init_system_timezone()`] has not been called yet.
pub(crate) fn system_timezone() -> &'static TimeZone {
    SYSTEM_TZ
        .get()
        .expect("init_system_timezone() should have been called")
}
