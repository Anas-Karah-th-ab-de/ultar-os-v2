//! What the ingestion side hands to the engine, and through what.
//!
//! Every delivery holds a [`Ticket`] of the share it came from
//! (`share.rs`): records, the counts of a source and the rejections of
//! unidentified peers alike. So nothing the ingestion side puts into the
//! queue can take room another source needs (review follow-up to round 3:
//! the drop *counts* of a flooding source used to fill the queue).

use crate::share::Ticket;
use std::sync::mpsc::{SyncSender, TrySendError};
use ultar_service::log::output::OutputLine;
use ultar_service::log::OwnedRecord;

/// What the ingestion side hands to the engine.
#[derive(Debug)]
pub enum Delivery {
    /// A valid record of `service`.
    Record {
        /// Verified name of the source.
        service: String,
        /// The record.
        record: OwnedRecord,
        /// The record's place in its source's share of the queue; given
        /// back when the delivery is dropped.
        ticket: Ticket,
    },
    /// A line of output of a service, from `ultar-init`.
    Output {
        /// The line.
        line: OutputLine,
        /// Place in the share of the output channel.
        ticket: Ticket,
    },
    /// Records of `service` that were refused or malformed since the last
    /// report.
    Counts {
        /// Verified name of the source.
        service: String,
        /// The counts.
        counts: Counts,
        /// Place in the source's share.
        ticket: Ticket,
    },
    /// Peers that could not be identified or exceeded a connection bound.
    Rejected {
        /// How many since the last report.
        count: u64,
        /// Place in the share of all rejections.
        ticket: Ticket,
    },
}

/// Records of one source that did not reach the engine, by reason.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Counts {
    /// Over the source's rate limit at the socket.
    pub rate: u64,
    /// The queue had no room (cannot happen while the shares fit the
    /// queue; counted so that it would show).
    pub queue: u64,
    /// Not a valid record.
    pub malformed: u64,
}

impl Counts {
    /// Whether there is anything to report.
    pub const fn is_empty(&self) -> bool {
        self.rate == 0 && self.queue == 0 && self.malformed == 0
    }
}

/// Where deliveries go.
pub trait Sink: Send + Sync {
    /// Offers one delivery without blocking: `Ok(true)` taken, `Ok(false)`
    /// no room right now, `Err(())` the receiver is gone for good.
    #[allow(clippy::result_unit_err)]
    fn offer(&self, delivery: Delivery) -> Result<bool, ()>;

    /// Delivers, waiting for room if necessary (used for counts only).
    /// `Err(())` when the receiver is gone for good.
    #[allow(clippy::result_unit_err)]
    fn deliver(&self, delivery: Delivery) -> Result<(), ()>;
}

impl Sink for SyncSender<Delivery> {
    fn offer(&self, delivery: Delivery) -> Result<bool, ()> {
        match self.try_send(delivery) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => Err(()),
        }
    }

    fn deliver(&self, delivery: Delivery) -> Result<(), ()> {
        self.send(delivery).map_err(|_| ())
    }
}
