//! Abstraction over the wall clock so that time-dependent logic can be tested
//! deterministically.

use chrono::{DateTime, Utc};

/// Source of the current wall-clock time.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// The real system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
pub mod testing {
    use super::Clock;
    use chrono::{DateTime, Duration, Utc};
    use std::sync::Mutex;

    /// A clock that only moves when told to.
    #[derive(Debug)]
    pub struct ManualClock(Mutex<DateTime<Utc>>);

    impl ManualClock {
        pub fn new(start: DateTime<Utc>) -> Self {
            Self(Mutex::new(start))
        }

        pub fn advance(&self, by: Duration) {
            let mut now = self.0.lock().expect("clock mutex poisoned");
            *now += by;
        }

        pub fn set(&self, to: DateTime<Utc>) {
            *self.0.lock().expect("clock mutex poisoned") = to;
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> DateTime<Utc> {
            *self.0.lock().expect("clock mutex poisoned")
        }
    }
}
