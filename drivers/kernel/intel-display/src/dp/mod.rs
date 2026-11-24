//! DisplayPort: AUX channel, DPCD, link configuration and link training.

pub mod aux;
pub mod dpcd;
pub mod link;
pub mod training;

#[cfg(test)]
pub mod sink;
