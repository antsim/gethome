//! Timer engine: day state, pure computation and Toggl synchronisation.

pub mod calendar;
pub mod compute;
pub mod scheduler;
pub mod state;
pub mod summary;

pub use compute::{compute, TimerViewModel};
pub use scheduler::{ControlError, EngineSnapshot, PollConfig, RefreshOutcome, SyncEngine};
pub use state::{SyncWarning, TimerStatus};
pub use summary::{day_summary, DaySummary};
