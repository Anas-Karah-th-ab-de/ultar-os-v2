//! The tools on the real system: each one is a typed call on the system bus, made with
//! the helper's own identity. What the bus policy does not allow the helper, it cannot do.

use crate::model::{Thinker, MAX_TOKENS};
use crate::tools::{self, Call, Outcome, Risk, Toolbox};
use std::time::Duration;
use ultar_bus::messages::agent::{Brain, Progress, StepOutcome};
use ultar_bus::messages::audio::{AudioOutcome, SetMute, SetVolume};
use ultar_bus::messages::launcher::{launcher_id, List, Role, Start};
use ultar_bus::messages::llm::Complete;
use ultar_bus::messages::llm_cloud::CloudComplete;
use ultar_bus::messages::mesh::{HandoffSend, MeshState, SendFile, SendOutcome};
use ultar_bus::messages::mesh_model::Think;
use ultar_bus::messages::mesh_remote::Act;
use ultar_bus::messages::net::{NetOutcome, SetWifi};
use ultar_bus::messages::power::{ActionOutcome, PowerOff, Restart};
use ultar_bus::messages::sysdialog::{self, AuthResult, Authenticate};
use ultar_bus::{Client, Text};

pub(crate) const CALL_TIME: Duration = Duration::from_secs(5);
/// How long another device's helper may take.
const REMOTE_TIME: Duration = Duration::from_secs(155);
/// How long a model may take for one answer.
pub const THINK_TIME: Duration = Duration::from_secs(125);
/// The helper's app id in the trusted dialog.
pub const APP_ID: &str = "org.ultar.helper";

/// The tools on the bus.
pub struct BusTools<'a> {
    /// The helper's connection.
    pub client: &'a Client,
    /// What the helper reaches now, in words for the model.
    pub reach: String,
    /// What the helper did lately, in words for the model.
    pub lately: String,
}

pub(crate) fn done(ok: bool, told: &str) -> Outcome {
    let outcome = if ok {
        StepOutcome::Done
    } else {
        StepOutcome::Failed
    };
    (outcome, told.to_owned())
}

pub(crate) fn flag(text: &str) -> Option<bool> {
    match text {
        "true" | "on" | "1" => Some(true),
        "false" | "off" | "0" => Some(false),
        _ => None,
    }
}

pub(crate) const BAD: &str = "bad argument";

impl BusTools<'_> {
    fn apps(&self) -> Outcome {
        match self.client.call(&List {}, CALL_TIME) {
            Ok(list) => {
                let ids: Vec<String> = list.apps.into_iter().map(|a| a.app).collect();
                done(true, &ids.join(", "))
            }
            Err(_) => done(false, "the app list is not reachable"),
        }
    }

    fn open(&self, app: &str) -> Outcome {
        let start = Start {
            app: launcher_id(app).to_owned(),
            role: Role::Ui,
            activation_token: None,
            uri: None,
            new_window: false,
        };
        done(self.client.call(&start, CALL_TIME).is_ok(), "")
    }

    fn volume(&self, percent: &str) -> Outcome {
        let Some(percent) = percent.parse::<u8>().ok().filter(|p| *p <= 100) else {
            return done(false, BAD);
        };
        let reply = self.client.call(&SetVolume { percent }, CALL_TIME);
        done(reply.is_ok_and(|r| r.outcome == AudioOutcome::Done), "")
    }

    fn mute(&self, on: &str) -> Outcome {
        let Some(on) = flag(on) else {
            return done(false, BAD);
        };
        let reply = self.client.call(&SetMute { on }, CALL_TIME);
        done(reply.is_ok_and(|r| r.outcome == AudioOutcome::Done), "")
    }

    fn wifi(&self, on: &str) -> Outcome {
        let Some(on) = flag(on) else {
            return done(false, BAD);
        };
        let reply = self.client.call(&SetWifi { on }, CALL_TIME);
        done(reply.is_ok_and(|r| r.outcome == NetOutcome::Done), "")
    }

    fn devices(&self) -> Outcome {
        let state = self
            .client
            .subscribe::<MeshState>()
            .and_then(|s| s.recv_timeout(CALL_TIME));
        let Ok(state) = state else {
            return done(false, "the mesh is not reachable");
        };
        let list: Vec<String> = state
            .peers
            .iter()
            .filter(|p| p.paired)
            .map(|p| {
                let how = if p.reachable {
                    "connected"
                } else {
                    "not connected"
                };
                format!("{} = {} ({how})", p.id, p.name)
            })
            .collect();
        done(true, &list.join("; "))
    }

    fn offered(outcome: Result<SendOutcome, ultar_bus::BusError>) -> Outcome {
        match outcome {
            Ok(SendOutcome::Offered { target, .. }) => done(
                true,
                &format!("offered to {}; it asks its person", target.name),
            ),
            Ok(SendOutcome::Choose(targets)) => {
                let list: Vec<String> = targets
                    .iter()
                    .map(|t| format!("{} = {}", t.device, t.name))
                    .collect();
                done(
                    false,
                    &format!("several devices, name one: {}", list.join("; ")),
                )
            }
            Ok(SendOutcome::NoTarget) => done(false, "no paired device can take it now"),
            Ok(SendOutcome::NoActivity) => done(false, "nothing is open that can be handed over"),
            Err(_) => done(false, "the mesh is not reachable"),
        }
    }

    fn continue_on(&self, device: &str) -> Outcome {
        let target = (!device.is_empty()).then(|| device.to_owned());
        Self::offered(self.client.call(&HandoffSend { target }, CALL_TIME))
    }

    fn send_file(&self, device: &str, path: &str) -> Outcome {
        let send = SendFile {
            device: device.to_owned(),
            path: path.to_owned(),
        };
        Self::offered(self.client.call(&send, CALL_TIME))
    }

    fn ask_device(&self, device: &str, request: &str) -> Outcome {
        let act = Act {
            device: device.to_owned(),
            text: request.to_owned(),
        };
        match self.client.call(&act, REMOTE_TIME) {
            Ok(reply) if reply.ok => {
                let steps: Vec<String> = reply
                    .steps
                    .iter()
                    .map(|s| format!("{} {:?}", s.tool, s.outcome))
                    .collect();
                let all = reply.steps.iter().all(|s| s.outcome == StepOutcome::Done);
                done(all && !steps.is_empty(), &steps.join("; "))
            }
            Ok(_) => done(false, "that device is not connected or does not allow it"),
            Err(_) => done(false, "the mesh is not reachable"),
        }
    }

    /// The person confirms a dangerous action in the system dialog (an administrator's
    /// password); without that nothing happens.
    pub(crate) fn confirmed(&self, tool: &str) -> bool {
        // whoever watches sees that this step waits for the person
        let _ = self.client.publish(&Progress::Waiting(tool.to_owned()));
        let ask = Authenticate {
            app_id: APP_ID.to_owned(),
            app_name: "helper".to_owned(),
            action: Text::key(&format!("agent.action.{tool}")),
        };
        let wait = Duration::from_millis(sysdialog::CALL_TIMEOUT_MS);
        let reply = self.client.call(&ask, wait);
        reply.is_ok_and(|r| r.result == AuthResult::Authorized)
    }

    fn power(&self, tool: &str) -> Outcome {
        let reply = match tool {
            "restart" => self.client.call(&Restart {}, CALL_TIME),
            _ => self.client.call(&PowerOff {}, CALL_TIME),
        };
        done(reply.is_ok_and(|r| r.outcome == ActionOutcome::Done), "")
    }
}

impl Toolbox for BusTools<'_> {
    fn note(&self, progress: &Progress) {
        let _ = self.client.publish(progress);
    }

    fn run(&self, call: &Call) -> Outcome {
        let Some(tool) = tools::find(&call.tool) else {
            return (StepOutcome::NotAllowed, "no such tool".to_owned());
        };
        if tool.risk == Risk::Dangerous && !self.confirmed(tool.name) {
            return (
                StepOutcome::NotConfirmed,
                "the person did not confirm".to_owned(),
            );
        }
        if let Some(outcome) = self.run_system(tool.name, call) {
            return outcome;
        }
        match tool.name {
            "list_apps" => self.apps(),
            "open_app" => self.open(call.arg("app")),
            "set_volume" => self.volume(call.arg("percent")),
            "mute" => self.mute(call.arg("on")),
            "wifi" => self.wifi(call.arg("on")),
            "devices" => self.devices(),
            "continue_on" => self.continue_on(call.arg("device")),
            "send_file" => self.send_file(call.arg("device"), call.arg("path")),
            "ask_device" => self.ask_device(call.arg("device"), call.arg("request")),
            "bluetooth" => self.bluetooth(call.arg("on")),
            "airplane" => self.airplane(call.arg("on")),
            "media" => self.media(call.arg("button")),
            "screenshot" => self.screenshot(),
            "lock" => self.lock(),
            "help" => Self::help(call.arg("topic")),
            "suspend" => self.suspend(),
            _ => self.power(tool.name),
        }
    }
}

/// The model on the bus: this device's, or a paired device's through the mesh.
pub struct BusThinker<'a> {
    /// The helper's connection.
    pub client: &'a Client,
    /// Where to think.
    pub brain: Brain,
}

impl Thinker for BusThinker<'_> {
    fn think(&self, system: &str, prompt: &str) -> Option<String> {
        let (system, prompt) = (system.to_owned(), prompt.to_owned());
        match &self.brain {
            Brain::Minimal => None,
            Brain::Local => {
                let ask = Complete {
                    system,
                    prompt,
                    max_tokens: MAX_TOKENS,
                };
                let reply = self.client.call(&ask, THINK_TIME).ok()?;
                reply.ok.then_some(reply.text)
            }
            Brain::Cloud => {
                let ask = CloudComplete {
                    system,
                    prompt,
                    max_tokens: MAX_TOKENS,
                };
                let reply = self.client.call(&ask, THINK_TIME).ok()?;
                reply.ok.then_some(reply.text)
            }
            Brain::Mesh(device) => {
                let ask = Think {
                    device: Some(device.clone()),
                    system,
                    prompt,
                    max_tokens: MAX_TOKENS,
                };
                let reply = self.client.call(&ask, THINK_TIME).ok()?;
                reply.ok.then_some(reply.text)
            }
        }
    }
}
