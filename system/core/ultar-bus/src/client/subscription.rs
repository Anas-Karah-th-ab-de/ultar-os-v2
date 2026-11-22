//! Receiving side of publish/subscribe.

use super::link::Inner;
use crate::error::{BusError, ErrorCode};
use crate::message::{decode_versioned, Topic};
use crate::topic::Pattern;
use crate::wire::{Envelope, Kind};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

/// One received publication, not yet decoded.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RawEvent {
    /// Topic the event was published on.
    pub topic: String,
    /// Contract version of the payload.
    pub version: u16,
    /// `true` when this is the stored last value replayed at subscribe time.
    pub retained: bool,
    /// Serialised message.
    pub payload: Vec<u8>,
}

impl RawEvent {
    /// Decodes the payload as `T`, checking topic and contract version.
    pub fn decode<T: Topic>(&self) -> Result<T, BusError> {
        if self.topic != T::TOPIC {
            return Err(BusError::InvalidName);
        }
        decode_versioned(self.version, &self.payload)
    }
}

/// What travels from the reader thread to a [`Subscription`].
pub(crate) enum SubscriptionItem {
    Event(RawEvent),
    Lagged,
    Failed(ErrorCode),
}

/// Reader-thread side of a subscription.
pub(crate) struct SubscriptionSlot {
    pub(crate) id: u64,
    pub(crate) pattern: Pattern,
    pub(crate) sender: SyncSender<SubscriptionItem>,
    pub(crate) lost: Arc<AtomicU64>,
}

impl SubscriptionSlot {
    /// Hands an item to the subscriber without ever blocking; an item that
    /// does not fit is counted as lost.
    pub(crate) fn push(&self, item: SubscriptionItem) {
        if self.sender.try_send(item).is_err() {
            self.lost.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// A live subscription to a topic or prefix. Dropping it unsubscribes.
///
/// Events of one topic arrive in the order the broker accepted them. When
/// events were lost – the local buffer overflowed, or the broker dropped the
/// connection as too slow – the next receive call reports
/// [`BusError::Lagged`] once and the stream then continues.
pub struct Subscription {
    pub(crate) id: u64,
    pub(crate) inner: Arc<Inner>,
    pub(crate) receiver: Receiver<SubscriptionItem>,
    pub(crate) lost: Arc<AtomicU64>,
}

impl Subscription {
    fn take_lost(&self) -> Option<BusError> {
        match self.lost.swap(0, Ordering::SeqCst) {
            0 => None,
            lost => Some(BusError::Lagged(lost)),
        }
    }

    fn unpack(&self, item: SubscriptionItem) -> Result<RawEvent, BusError> {
        match item {
            SubscriptionItem::Event(event) => Ok(event),
            SubscriptionItem::Lagged => {
                Err(BusError::Lagged(self.lost.swap(0, Ordering::SeqCst).max(1)))
            }
            SubscriptionItem::Failed(code) => Err(BusError::Remote(code)),
        }
    }

    /// Blocks until the next event arrives.
    pub fn recv(&self) -> Result<RawEvent, BusError> {
        if let Some(lagged) = self.take_lost() {
            return Err(lagged);
        }
        match self.receiver.recv() {
            Ok(item) => self.unpack(item),
            Err(_) => Err(BusError::Closed),
        }
    }

    /// Waits at most `timeout` for the next event.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<RawEvent, BusError> {
        if let Some(lagged) = self.take_lost() {
            return Err(lagged);
        }
        match self.receiver.recv_timeout(timeout) {
            Ok(item) => self.unpack(item),
            Err(RecvTimeoutError::Timeout) => Err(BusError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(BusError::Closed),
        }
    }

    /// Returns an event if one is already waiting.
    pub fn try_recv(&self) -> Result<Option<RawEvent>, BusError> {
        if let Some(lagged) = self.take_lost() {
            return Err(lagged);
        }
        match self.receiver.try_recv() {
            Ok(item) => self.unpack(item).map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(BusError::Closed),
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        crate::sync::lock(&self.inner.subscriptions).retain(|slot| slot.id != self.id);
        let _ = self
            .inner
            .send(&Envelope::new(Kind::Unsubscribe, self.id), None);
    }
}

/// A subscription that decodes every event as the topic type `T`.
pub struct TypedSubscription<T: Topic> {
    raw: Subscription,
    marker: PhantomData<fn() -> T>,
}

impl<T: Topic> TypedSubscription<T> {
    pub(crate) fn new(raw: Subscription) -> Self {
        Self {
            raw,
            marker: PhantomData,
        }
    }

    /// Blocks until the next message arrives.
    pub fn recv(&self) -> Result<T, BusError> {
        self.raw.recv()?.decode()
    }

    /// Waits at most `timeout` for the next message.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, BusError> {
        self.raw.recv_timeout(timeout)?.decode()
    }

    /// Returns a message if one is already waiting.
    pub fn try_recv(&self) -> Result<Option<T>, BusError> {
        self.raw.try_recv()?.map(|event| event.decode()).transpose()
    }

    /// The untyped subscription underneath.
    pub fn into_raw(self) -> Subscription {
        self.raw
    }
}
