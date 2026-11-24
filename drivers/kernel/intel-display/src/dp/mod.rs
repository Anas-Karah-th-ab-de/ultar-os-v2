//! DisplayPort: AUX channel, DPCD, link configuration and link training.

#[path = "aux_channel.rs"]
pub mod aux; // file name avoids the reserved Windows name "aux"
pub mod dpcd;
pub mod link;
pub mod training;

#[cfg(test)]
pub mod sink;
