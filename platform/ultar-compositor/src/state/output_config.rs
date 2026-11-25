//! Requests of the display service (right `ui.output-config`): setups,
//! confirmation, display numbers, looking for displays. The state checks
//! rights and queues; the loop (`server/outputs.rs`) switches the backend's
//! modes, applies, and goes back when a provisional setup is not confirmed.

use super::{ClientId, State};
use crate::output_setup::Snapshot;
use std::collections::VecDeque;
use ultar_surface::proto::{
    ErrorCode, Event, OutputId, Request, Rights, Setup, SetupOutcome, SetupRefusal,
};

/// Setups waiting for the loop at once (more: refused as busy).
pub const MAX_QUEUED_SETUPS: usize = 4;

/// A setup in effect that goes back unless confirmed.
#[derive(Clone, Debug)]
pub struct Provisional {
    /// The client that set it (its end takes the setup back).
    pub owner: ClientId,
    /// Serial of the setup.
    pub serial: u32,
    /// Time (state clock, ms) when it goes back.
    pub deadline: u64,
    /// The arrangement before the first unconfirmed setup.
    pub previous: Snapshot,
}

/// Display numbers shown large on each output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identify {
    /// Output and number.
    pub numbers: Vec<(OutputId, u8)>,
    /// Shown until this time (ms).
    pub until: u64,
}

/// Output management state.
#[derive(Debug, Default)]
pub struct OutputManagement {
    /// The setup waiting for confirmation.
    pub provisional: Option<Provisional>,
    /// Numbers on the displays.
    pub identify: Option<Identify>,
    /// Setups for the loop to apply.
    pub(crate) queued: VecDeque<(ClientId, Setup)>,
    /// Clients that asked to look for displays.
    pub(crate) detect: Vec<ClientId>,
    /// The provisional setup must go back (and why).
    pub(crate) revert_due: Option<SetupOutcome>,
}

impl State {
    /// One of the output requests, or the pointer and key-repeat configuration of the same
    /// right.
    pub(super) fn output_request(&mut self, client: ClientId, r: Request) -> Result<(), ErrorCode> {
        match r {
            Request::ConfigureOutputs { setup } => self.configure_outputs(client, setup),
            Request::ConfirmOutputs { serial } => self.confirm_outputs(client, serial),
            Request::IdentifyOutputs { numbers, ms } => self.identify_outputs(client, numbers, ms),
            Request::DetectOutputs => self.detect_outputs(client),
            Request::ConfigurePointer {
                speed,
                natural_scroll,
                swap_buttons,
                acceleration,
                size,
                double_click_ms,
            } => {
                self.require_output_config(client)?;
                let old_size = self.pointer.size;
                let pointer = &mut self.pointer;
                pointer.speed = speed;
                pointer.natural_scroll = natural_scroll;
                pointer.swap_buttons = swap_buttons;
                pointer.accel.on = acceleration;
                pointer.size = size;
                pointer.double_click_ms = double_click_ms;
                if old_size != size {
                    // The pointer image changes under the resting pointer: redraw
                    // what the old size covered and what the new one covers.
                    self.damage_cursor_sized(old_size);
                    self.damage_cursor();
                }
                Ok(())
            }
            Request::ConfigureKeyRepeat {
                enabled,
                delay_ms,
                per_second,
            } => {
                self.require_output_config(client)?;
                let repeat = &mut self.keyboard.repeat;
                (repeat.enabled, repeat.delay_ms, repeat.per_second) =
                    (enabled, delay_ms, per_second);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn require_output_config(&self, client: ClientId) -> Result<(), ErrorCode> {
        match self.clients.get(&client) {
            Some(c) if c.rights.contains(Rights::OUTPUT_CONFIG) => Ok(()),
            Some(_) => Err(ErrorCode::Permission),
            None => Err(ErrorCode::UnknownObject),
        }
    }

    /// `ConfigureOutputs`: queued for the loop unless another client's
    /// setup waits for confirmation.
    pub(super) fn configure_outputs(
        &mut self,
        client: ClientId,
        setup: Setup,
    ) -> Result<(), ErrorCode> {
        self.require_output_config(client)?;
        let manage = &self.session.manage;
        let foreign = manage
            .provisional
            .as_ref()
            .is_some_and(|p| p.owner != client);
        if foreign || manage.queued.len() >= MAX_QUEUED_SETUPS {
            let serial = setup.serial;
            self.send(
                client,
                Event::OutputSetupDone {
                    serial,
                    outcome: SetupOutcome::Refused(SetupRefusal::Busy),
                },
            );
            return Ok(());
        }
        self.session.manage.queued.push_back((client, setup));
        Ok(())
    }

    /// `ConfirmOutputs`: the provisional setup of this client becomes final.
    pub(super) fn confirm_outputs(
        &mut self,
        client: ClientId,
        serial: u32,
    ) -> Result<(), ErrorCode> {
        self.require_output_config(client)?;
        let mine = self
            .session
            .manage
            .provisional
            .as_ref()
            .is_some_and(|p| p.owner == client && p.serial == serial);
        let outcome = if mine {
            self.session.manage.provisional = None;
            SetupOutcome::Confirmed
        } else {
            SetupOutcome::Refused(SetupRefusal::Invalid)
        };
        self.send(client, Event::OutputSetupDone { serial, outcome });
        Ok(())
    }

    /// `IdentifyOutputs`: numbers on the connected outputs for `ms`.
    pub(super) fn identify_outputs(
        &mut self,
        client: ClientId,
        numbers: Vec<(OutputId, u8)>,
        ms: u32,
    ) -> Result<(), ErrorCode> {
        self.require_output_config(client)?;
        let numbers: Vec<_> = numbers
            .into_iter()
            .filter(|(o, _)| self.layout.get(*o).is_some())
            .collect();
        self.session.manage.identify = (!numbers.is_empty() && ms > 0).then(|| Identify {
            numbers,
            until: self.now + u64::from(ms),
        });
        self.damage_all();
        Ok(())
    }

    /// `DetectOutputs`: the loop rescans and answers.
    pub(super) fn detect_outputs(&mut self, client: ClientId) -> Result<(), ErrorCode> {
        self.require_output_config(client)?;
        if !self.session.manage.detect.contains(&client) {
            self.session.manage.detect.push(client);
        }
        Ok(())
    }

    /// Sends every output's details to one client (output-config right only).
    pub fn send_output_details(&mut self, client: ClientId) {
        if self.require_output_config(client).is_err() {
            return;
        }
        let ids: Vec<OutputId> = self.layout.outputs().iter().map(|o| o.id).collect();
        for id in ids {
            if let Some(d) = self.layout.details_of(id) {
                self.send(client, Event::OutputDetails(Box::new(d)));
            }
        }
    }

    /// Sends every output's details to every display service.
    pub fn broadcast_output_details(&mut self) {
        let ids: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.hello && c.rights.contains(Rights::OUTPUT_CONFIG))
            .map(|(&id, _)| id)
            .collect();
        for client in ids {
            self.send_output_details(client);
        }
    }

    /// A client ended: a provisional setup of its goes back at once.
    pub(super) fn output_client_gone(&mut self, client: ClientId) {
        let manage = &mut self.session.manage;
        manage.queued.retain(|(c, _)| *c != client);
        manage.detect.retain(|c| *c != client);
        if manage
            .provisional
            .as_ref()
            .is_some_and(|p| p.owner == client)
        {
            manage.revert_due = Some(SetupOutcome::RevertedTimeout);
        }
    }

    /// Expires the display numbers and the provisional deadline.
    pub fn tick_outputs(&mut self) {
        self.tick_visual();
        let now = self.now;
        if self
            .session
            .manage
            .identify
            .as_ref()
            .is_some_and(|i| i.until <= now)
        {
            self.session.manage.identify = None;
            self.damage_all();
        }
        let manage = &mut self.session.manage;
        if manage
            .provisional
            .as_ref()
            .is_some_and(|p| p.deadline <= now)
        {
            manage
                .revert_due
                .get_or_insert(SetupOutcome::RevertedTimeout);
        }
    }

    /// Earliest output-management deadline.
    pub fn output_deadline(&self) -> Option<u64> {
        let manage = &self.session.manage;
        let identify = manage.identify.as_ref().map(|i| i.until);
        let provisional = manage.provisional.as_ref().map(|p| p.deadline);
        let now_due =
            (manage.revert_due.is_some() || !manage.queued.is_empty() || !manage.detect.is_empty())
                .then_some(self.now);
        let flash =
            (self.session.visual.flash_until > self.now).then_some(self.session.visual.flash_until);
        [identify, provisional, now_due, flash]
            .into_iter()
            .flatten()
            .min()
    }
}
