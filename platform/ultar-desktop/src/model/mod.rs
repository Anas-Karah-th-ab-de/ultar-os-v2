//! State of the desktop without drawing: apps and windows, time and calendar, quick
//! settings, notifications, search. Everything here is plain data with pure functions, so
//! that the views and the click handling can be tested without a compositor.

pub mod apps;
pub mod arrange;
pub mod calc;
pub mod calendar;
pub mod characters;
pub mod helper;
pub mod history;
pub mod media;
pub mod mesh;
pub mod notes;
pub mod quick;
pub mod search;
pub mod services;
pub mod state;
pub mod time;
pub mod wifi;

pub use state::{BarPlace, Desktop, Form, Open, QuickPage, Shape};
