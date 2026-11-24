//! State of the desktop without drawing: apps and windows, time and calendar, quick
//! settings, notifications, search. Everything here is plain data with pure functions, so
//! that the views and the click handling can be tested without a compositor.

pub mod apps;
pub mod calendar;
pub mod notes;
pub mod quick;
pub mod search;
pub mod state;
pub mod time;

pub use state::{Desktop, Open, QuickPage};
