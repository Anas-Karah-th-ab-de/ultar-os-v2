//! What the broker does with each envelope a client sends.

use super::observe::{BrokerEvent, Operation};
use super::state::{ConnId, Pending, Queued, Retained, State};
use super::BrokerConfig;
use crate::error::ErrorCode;
use crate::policy::Policy;
use crate::topic::{is_valid_name, is_valid_segment_name, Pattern};
use crate::wire::{Envelope, Kind, FLAG_ACK, FLAG_RETAIN};
use std::sync::Arc;
use std::time::Instant;

/// Read-only inputs of the routing step.
pub(crate) struct Rules<'a> {
    pub(crate) policy: &'a Policy,
    pub(crate) config: &'a BrokerConfig,
}

/// Handles one envelope from `conn`. An `Err` ends the connection with that
/// code; ordinary failures are answered with an `Error` envelope instead.
pub(crate) fn handle(
    state: &mut State,
    rules: &Rules<'_>,
    conn: ConnId,
    envelope: Envelope,
) -> Result<(), ErrorCode> {
    let id = envelope.id;
    let operation = match envelope.kind {
        Kind::Register => Operation::Register,
        Kind::Unregister => Operation::Unregister,
        Kind::Subscribe | Kind::Unsubscribe => Operation::Subscribe,
        Kind::Event => Operation::Publish,
        _ => Operation::Call,
    };
    let outcome = match envelope.kind {
        Kind::Register => register(state, rules, conn, &envelope),
        Kind::Unregister => unregister(state, conn, &envelope),
        Kind::Subscribe => subscribe(state, rules, conn, &envelope),
        Kind::Unsubscribe => unsubscribe(state, conn, &envelope),
        Kind::Event => return publish(state, rules, conn, envelope),
        Kind::Request => request(state, rules, conn, envelope),
        Kind::Response | Kind::Error => {
            forward_reply(state, conn, envelope);
            return Ok(());
        }
        Kind::Hello | Kind::Welcome => return Err(ErrorCode::ProtocolViolation),
    };
    if let Err(code) = outcome {
        refused(state, conn, operation, code);
        state.reply_error(conn, id, code);
    }
    Ok(())
}

/// Reports a refused operation to the observer.
fn refused(state: &State, conn: ConnId, operation: Operation, code: ErrorCode) {
    if let Some(entry) = state.conns.get(&conn) {
        state.observe(&BrokerEvent::Refused {
            identity: entry.identity.as_str(),
            operation,
            code,
        });
    }
}

fn register(
    state: &mut State,
    rules: &Rules<'_>,
    conn: ConnId,
    envelope: &Envelope,
) -> Result<(), ErrorCode> {
    let service = envelope.target.as_str();
    if !is_valid_name(service) {
        return Err(ErrorCode::InvalidName);
    }
    let entry = state.conns.get(&conn).ok_or(ErrorCode::ProtocolViolation)?;
    if !rules.policy.may_register(&entry.identity, service) {
        return Err(ErrorCode::PermissionDenied);
    }
    match state.services.get(service) {
        Some(owner) if *owner != conn => return Err(ErrorCode::NameTaken),
        Some(_) => {}
        None if entry.services.len() >= rules.config.max_services_per_connection => {
            return Err(ErrorCode::LimitExceeded);
        }
        None => {
            state.services.insert(service.to_owned(), conn);
            if let Some(entry) = state.conns.get_mut(&conn) {
                entry.services.push(service.to_owned());
            }
        }
    }
    state.ack(conn, envelope.id);
    Ok(())
}

fn unregister(state: &mut State, conn: ConnId, envelope: &Envelope) -> Result<(), ErrorCode> {
    if state.services.get(&envelope.target) != Some(&conn) {
        return Err(ErrorCode::UnknownService);
    }
    state.services.remove(&envelope.target);
    if let Some(entry) = state.conns.get_mut(&conn) {
        entry.services.retain(|service| *service != envelope.target);
    }
    state.ack(conn, envelope.id);
    Ok(())
}

fn subscribe(
    state: &mut State,
    rules: &Rules<'_>,
    conn: ConnId,
    envelope: &Envelope,
) -> Result<(), ErrorCode> {
    let pattern = Pattern::parse(&envelope.target).map_err(|_| ErrorCode::InvalidName)?;
    let entry = state
        .conns
        .get_mut(&conn)
        .ok_or(ErrorCode::ProtocolViolation)?;
    if !rules.policy.may_subscribe(&entry.identity, &pattern) {
        return Err(ErrorCode::PermissionDenied);
    }
    entry.subscriptions.retain(|(id, _)| *id != envelope.id);
    if entry.subscriptions.len() >= rules.config.max_subscriptions {
        return Err(ErrorCode::LimitExceeded);
    }
    entry.subscriptions.push((envelope.id, pattern.clone()));
    state.ack(conn, envelope.id);
    replay_retained(state, conn, envelope.id, &pattern);
    Ok(())
}

/// Sends the stored last values of all topics `pattern` matches, addressed
/// to the one subscription that asked for them.
fn replay_retained(state: &mut State, conn: ConnId, subscription: u64, pattern: &Pattern) {
    let replays: Vec<Envelope> = state
        .retained
        .iter()
        .filter(|(topic, _)| pattern.matches(topic))
        .map(|(topic, retained)| {
            let mut event = Envelope::new(Kind::Event, subscription)
                .with_target(topic)
                .with_payload(retained.version, retained.payload.clone());
            event.flags = FLAG_RETAIN;
            event
        })
        .collect();
    for event in &replays {
        state.send(conn, event);
    }
}

fn unsubscribe(state: &mut State, conn: ConnId, envelope: &Envelope) -> Result<(), ErrorCode> {
    if let Some(entry) = state.conns.get_mut(&conn) {
        entry.subscriptions.retain(|(id, _)| *id != envelope.id);
    }
    state.ack(conn, envelope.id);
    Ok(())
}

fn check_publish(
    state: &State,
    rules: &Rules<'_>,
    conn: ConnId,
    envelope: &Envelope,
) -> Result<(), ErrorCode> {
    if !is_valid_name(&envelope.target) {
        return Err(ErrorCode::InvalidName);
    }
    let entry = state.conns.get(&conn).ok_or(ErrorCode::ProtocolViolation)?;
    if !rules.policy.may_publish(&entry.identity, &envelope.target) {
        return Err(ErrorCode::PermissionDenied);
    }
    let retain = envelope.flags & FLAG_RETAIN != 0;
    if retain && !state.retained.contains_key(&envelope.target) {
        let own = state
            .retained
            .values()
            .filter(|r| r.owner == entry.identity.as_str())
            .count();
        if state.retained.len() >= rules.config.max_retained
            || own >= rules.config.max_retained_per_identity
        {
            return Err(ErrorCode::LimitExceeded);
        }
    }
    Ok(())
}

fn publish(
    state: &mut State,
    rules: &Rules<'_>,
    conn: ConnId,
    envelope: Envelope,
) -> Result<(), ErrorCode> {
    let wants_ack = envelope.flags & FLAG_ACK != 0;
    if let Err(code) = check_publish(state, rules, conn, &envelope) {
        refused(state, conn, Operation::Publish, code);
        // Unconfirmed publications have nobody waiting for an answer.
        if wants_ack {
            state.reply_error(conn, envelope.id, code);
        }
        return Ok(());
    }
    let receivers: Vec<ConnId> = state
        .conns
        .iter()
        .filter(|(_, entry)| {
            entry
                .subscriptions
                .iter()
                .any(|(_, p)| p.matches(&envelope.target))
        })
        .map(|(id, _)| *id)
        .collect();
    let id = envelope.id;
    let retain = envelope.flags & FLAG_RETAIN != 0;
    let owner = state
        .conns
        .get(&conn)
        .map(|e| e.identity.as_str().to_owned())
        .unwrap_or_default();
    let event = Envelope {
        id: 0,
        flags: 0,
        origin: String::new(),
        method: String::new(),
        ..envelope
    };
    if !receivers.is_empty() {
        let frame = Arc::new(event.to_frame(u32::MAX).map_err(|_| ErrorCode::Malformed)?);
        for receiver in receivers {
            state.deliver(receiver, frame.clone());
        }
    }
    if retain {
        let retained = Retained {
            version: event.version,
            payload: event.payload,
            owner,
        };
        state.retained.insert(event.target, retained);
    }
    if wants_ack {
        state.ack(conn, id);
    }
    Ok(())
}

fn request(
    state: &mut State,
    rules: &Rules<'_>,
    conn: ConnId,
    envelope: Envelope,
) -> Result<(), ErrorCode> {
    if !is_valid_name(&envelope.target) || !is_valid_segment_name(&envelope.method) {
        return Err(ErrorCode::InvalidName);
    }
    let entry = state.conns.get(&conn).ok_or(ErrorCode::ProtocolViolation)?;
    if !rules
        .policy
        .may_call(&entry.identity, &envelope.target, &envelope.method)
    {
        return Err(ErrorCode::PermissionDenied);
    }
    let origin = entry.identity.as_str().to_owned();
    let service = *state
        .services
        .get(&envelope.target)
        .ok_or(ErrorCode::UnknownService)?;
    let broker_id = state.next_request;
    state.next_request = state.next_request.wrapping_add(1).max(1);
    let caller_id = envelope.id;
    let forwarded = Envelope {
        id: broker_id,
        flags: 0,
        origin,
        ..envelope
    };
    let frame = forwarded
        .to_frame(u32::MAX)
        .map_err(|_| ErrorCode::Malformed)?;
    match state.try_queue(service, Arc::new(frame)) {
        Queued::Yes => {}
        Queued::Full => return Err(ErrorCode::ServiceBusy),
        Queued::Gone => return Err(ErrorCode::ServiceGone),
    }
    let expires = Instant::now() + rules.config.pending_ttl;
    state.remember(
        broker_id,
        Pending {
            caller: conn,
            caller_id,
            service,
            expires,
        },
    );
    Ok(())
}

/// Routes a service's answer (or typed failure) back to the caller.
/// Answers nobody waits for (late, forged or duplicated) are discarded.
fn forward_reply(state: &mut State, conn: ConnId, envelope: Envelope) {
    let is_answer = state
        .pending
        .get(&envelope.id)
        .is_some_and(|p| p.service == conn);
    if !is_answer {
        return;
    }
    let Some(pending) = state.pending.remove(&envelope.id) else {
        return;
    };
    let reply = Envelope {
        id: pending.caller_id,
        target: String::new(),
        method: String::new(),
        origin: String::new(),
        ..envelope
    };
    state.send(pending.caller, &reply);
}
