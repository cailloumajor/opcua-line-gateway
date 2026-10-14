use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};
use tick::SimpleClock;

/// Represents a time provider to be used in this project.
#[derive(Clone)]
pub struct TimeProvider {
    /// Clock provider.
    clock: SimpleClock,
    /// System timezone.
    tz: TimeZone,
}

impl TimeProvider {
    /// Creates a new [`TimeProvider`] from anything that can be turned
    /// into a [`SimpleClock`], and a time zone.
    pub fn new<C>(clock: C, tz: TimeZone) -> Self
    where
        C: Into<SimpleClock>,
    {
        let clock = clock.into();

        Self { clock, tz }
    }

    /// Returns the current system time.
    pub(crate) fn now(&self) -> Timestamp {
        self.clock.system_time_as()
    }

    /// Returns the time zone aware current time.
    pub(crate) fn now_zoned(&self) -> Zoned {
        self.now().to_zoned(self.tz.clone())
    }
}

#[cfg(test)]
mod tests {
    use jiff::tz::Offset;
    use tick::ClockControl;

    use super::*;

    #[test]
    fn now() {
        let clock = ClockControl::new();
        let tz = TimeZone::fixed(Offset::constant(-5));
        let time_provider = TimeProvider::new(clock, tz);

        assert_eq!(time_provider.now(), Timestamp::UNIX_EPOCH);
    }

    #[test]
    fn now_zoned() {
        let clock = ClockControl::new();
        let tz = TimeZone::fixed(Offset::constant(-5));
        let time_provider = TimeProvider::new(clock, tz);

        let got = time_provider.now_zoned().to_string();

        assert_eq!(got, "1969-12-31T19:00:00-05:00[-05:00]");
    }
}
