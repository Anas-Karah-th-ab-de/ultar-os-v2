//! Serving side of request/response.

use super::link::Inner;
use crate::error::{BusError, ErrorCode};
use crate::message::{decode, decode_versioned, encode, Call, Message};
use crate::sync::lock;
use crate::wire::{Envelope, Kind};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

/// A request a client sent to a service registered on this connection.
///
/// Exactly one answer is sent: by [`Self::reply`], [`Self::reply_raw`],
/// [`Self::fail`] or [`Self::respond`] – or `NoReply` when the request is
/// dropped unanswered, so callers never wait for their full timeout.
pub struct IncomingRequest {
    inner: Arc<Inner>,
    generation: u64,
    id: u64,
    method: String,
    caller: String,
    version: u16,
    payload: Vec<u8>,
    answered: bool,
}

impl IncomingRequest {
    pub(crate) fn new(inner: Arc<Inner>, generation: u64, envelope: Envelope) -> Self {
        Self {
            inner,
            generation,
            id: envelope.id,
            method: envelope.method,
            caller: envelope.origin,
            version: envelope.version,
            payload: envelope.payload,
            answered: false,
        }
    }

    /// Marks the request as answered without sending anything and hands
    /// `code` back, for callers that send the refusal themselves.
    pub(crate) fn disarm(mut self, code: ErrorCode) -> ErrorCode {
        self.answered = true;
        code
    }

    /// Name of the called method.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Identity of the caller as authenticated by the broker.
    pub fn caller(&self) -> &str {
        &self.caller
    }

    /// Contract version the caller used (`0` for untyped requests).
    pub fn version(&self) -> u16 {
        self.version
    }

    /// The raw request payload.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Decodes the request as the call type `C`, checking method and version.
    pub fn decode<C: Call>(&self) -> Result<C, BusError> {
        if self.method != C::METHOD {
            return Err(BusError::Remote(ErrorCode::UnknownMethod));
        }
        decode_versioned(self.version, &self.payload)
    }

    /// Decodes the payload of an untyped request.
    pub fn decode_raw<T: DeserializeOwned>(&self) -> Result<T, BusError> {
        decode(&self.payload)
    }

    fn send(&mut self, envelope: &Envelope) -> Result<(), BusError> {
        self.answered = true;
        self.inner.send(envelope, Some(self.generation)).map(|_| ())
    }

    /// Answers with a typed message.
    pub fn reply<R: Message>(mut self, reply: &R) -> Result<(), BusError> {
        let payload = encode(reply)?;
        self.send(&Envelope::new(Kind::Response, self.id).with_payload(R::VERSION, payload))
    }

    /// Answers an untyped request.
    pub fn reply_raw<T: Serialize>(mut self, reply: &T) -> Result<(), BusError> {
        let payload = encode(reply)?;
        self.send(&Envelope::new(Kind::Response, self.id).with_payload(0, payload))
    }

    /// Answers with a typed failure.
    pub fn fail(mut self, code: ErrorCode) -> Result<(), BusError> {
        self.send(&Envelope::error(self.id, code))
    }

    /// Decodes the request as `C`, runs `handler` and sends its result.
    /// Requests that do not decode are answered with the matching error
    /// (`UnknownMethod`, `VersionMismatch` or `BadPayload`).
    pub fn respond<C: Call>(
        self,
        handler: impl FnOnce(C) -> Result<C::Reply, ErrorCode>,
    ) -> Result<(), BusError> {
        let call = match self.decode::<C>() {
            Ok(call) => call,
            Err(BusError::Malformed) => return self.fail(ErrorCode::BadPayload),
            Err(err) => return self.fail(err.code()),
        };
        match handler(call) {
            Ok(reply) => self.reply(&reply),
            Err(code) => self.fail(code),
        }
    }
}

impl Drop for IncomingRequest {
    fn drop(&mut self) {
        if !self.answered {
            let _ = self.send(&Envelope::error(self.id, ErrorCode::NoReply));
        }
    }
}

/// The inbox of a registered service. Dropping it unregisters the service.
///
/// The owner decides on which thread requests are handled: call
/// [`Self::recv`] in a loop, or hand the endpoint to [`Self::run`].
pub struct ServiceEndpoint {
    pub(crate) name: String,
    pub(crate) inner: Arc<Inner>,
    pub(crate) inbox: Receiver<IncomingRequest>,
}

impl ServiceEndpoint {
    /// Name the service is registered under.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Blocks until the next request arrives; `Closed` when the client ended.
    pub fn recv(&self) -> Result<IncomingRequest, BusError> {
        self.inbox.recv().map_err(|_| BusError::Closed)
    }

    /// Waits at most `timeout` for the next request.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<IncomingRequest, BusError> {
        self.inbox.recv_timeout(timeout).map_err(|err| match err {
            RecvTimeoutError::Timeout => BusError::Timeout,
            RecvTimeoutError::Disconnected => BusError::Closed,
        })
    }

    /// Returns a request if one is already waiting.
    pub fn try_recv(&self) -> Result<Option<IncomingRequest>, BusError> {
        match self.inbox.try_recv() {
            Ok(request) => Ok(Some(request)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(BusError::Closed),
        }
    }

    /// Handles requests with `handler` until the client is closed.
    pub fn run(self, mut handler: impl FnMut(IncomingRequest)) {
        while let Ok(request) = self.recv() {
            handler(request);
        }
    }
}

impl Drop for ServiceEndpoint {
    fn drop(&mut self) {
        lock(&self.inner.services).remove(&self.name);
        let unregister =
            Envelope::new(Kind::Unregister, self.inner.next_id()).with_target(&self.name);
        let _ = self.inner.send(&unregister, None);
    }
}
