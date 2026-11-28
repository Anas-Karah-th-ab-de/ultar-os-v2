//! The bus side of the service for apps from other systems (`messages::foreign`).
//!
//! Calls are answered at once; anything that takes time (reading a package, the
//! administrator's confirmation, the package manager) runs as one job on a worker
//! thread – one job at a time – and reports on `foreign.progress`.

use crate::jobs::{self, Ctx, Request};
use crate::layout::{size_of, Layout};
use crate::state::State;
use std::path::PathBuf;
use ultar_bus::messages::foreign::{
    Changed, Failure, Inspect, InspectReply, Install, InstallFrom, InstallOutcome, InstallReply,
    List, ListReply, Outcome, OutcomeReply, Remove, RuntimeState, RuntimeStateReply, RuntimeUpdate,
    SetChoices, Source, Step, MAX_APPS, SERVICE,
};
use ultar_bus::{Call, Client, ErrorCode, IncomingRequest};
use ultar_service::log::{Level, Record};
use ultar_svckit::{Bus, Service};

/// The service.
pub struct ForeignService {
    ctx: Ctx,
    state: State,
    /// A job runs.
    busy: bool,
    next_job: u64,
    /// The bytes of the Linux area, counted once (a walk over the whole tree is slow on a
    /// small device) and counted again after anything that changes the area.
    runtime_size: std::cell::Cell<Option<u64>>,
}

impl ForeignService {
    /// The service with its places below `root`; `helper`: this program; `sources`:
    /// where files may come from (`detect::SOURCES` on a device).
    pub fn new(root: PathBuf, helper: PathBuf, sources: Vec<PathBuf>) -> Self {
        let layout = Layout::new(&root);
        // a damaged state is kept aside (never overwritten by an empty one): the launcher
        // still knows the apps, an administrator can look at it
        let state = State::load(&layout.state()).unwrap_or_else(|_| {
            let _ = std::fs::rename(layout.state(), layout.state().with_extension("damaged"));
            State::default()
        });
        Self {
            ctx: Ctx {
                layout,
                helper,
                sources,
                trusted: PathBuf::from(ultar_pkg::TRUSTED_DIR),
            },
            state,
            busy: false,
            next_job: 1,
            runtime_size: std::cell::Cell::new(None),
        }
    }

    /// Checks components against the keys in `dir` (tests; a device has its built-in ones).
    #[must_use]
    pub fn with_trusted_keys(mut self, dir: PathBuf) -> Self {
        self.ctx.trusted = dir;
        self
    }

    /// Installs or replaces the Linux area from a signed component on a drive or in the
    /// areas (`component.rs`); an area holding the person's programs is never replaced.
    fn runtime_update(&mut self, caller: String, bus: &Bus<'_, Self>) -> InstallOutcome {
        if self.busy {
            return InstallOutcome::Busy;
        }
        let job = self.job_id();
        let ctx = self.ctx.clone();
        // `.deb` programs live in the area, Windows programs run with its Wine
        let holds_apps = !self.state.apps.is_empty();
        self.busy = bus.later(move |client| {
            let result = crate::component::update(&ctx, client, job, &caller, holds_apps)
                .map(|u| match u {
                    crate::component::Updated::Cancelled => None,
                    crate::component::Updated::Installed(_) | crate::component::Updated::Unchanged => {
                        Some(Vec::new())
                    }
                });
            Box::new(move |s: &mut ForeignService, bus| s.finished(job, result, bus))
        });
        if self.busy {
            InstallOutcome::Started(job)
        } else {
            InstallOutcome::Busy
        }
    }

    fn job_id(&mut self) -> u64 {
        let id = self.next_job;
        self.next_job += 1;
        id
    }

    fn save(&self, bus: &Bus<'_, Self>) -> bool {
        self.runtime_size.set(None);
        let saved = self.state.save(&self.ctx.layout.state()).is_ok();
        if !saved {
            bus.log(Record::new(Level::Error, "foreign.state_not_saved"));
        }
        bus.publish(&Changed {});
        saved
    }

    fn install(&mut self, install: Install, caller: String, bus: &Bus<'_, Self>) -> InstallOutcome {
        if !install.choices.is_valid() {
            return InstallOutcome::Invalid;
        }
        if self.busy {
            return InstallOutcome::Busy;
        }
        if !self.ctx.layout.runtime_installed() {
            return InstallOutcome::RuntimeMissing;
        }
        let path = match install.from {
            InstallFrom::File(path) if crate::detect::path_ok(&path) => path,
            InstallFrom::File(_) => return InstallOutcome::Invalid,
            // packages of the sources need the network, which the installation cell gets
            // only with the kernel's barrier (stage P0, docs/FREMD-APPS.md)
            InstallFrom::Archive(name) => {
                return self.fail_now(&name, Failure::Network, bus);
            }
        };
        let windows = match crate::detect::open_source(&self.ctx.sources, &path).map(|o| o.source) {
            Ok(Source::Deb) => false,
            Ok(Source::Exe | Source::Msi) if self.ctx.layout.windows_installed() => true,
            Ok(Source::Unknown) | Err(_) => return InstallOutcome::Invalid,
            Ok(_) => return InstallOutcome::NotAvailable,
        };
        let job = self.job_id();
        let (ctx, state) = (self.ctx.clone(), self.state.clone());
        let request = Request {
            job,
            caller,
            path,
            choices: install.choices,
        };
        self.busy = bus.later(move |client| {
            let result = if windows {
                crate::windows::install(&ctx, client, &state, &request)
            } else {
                jobs::install_file(&ctx, client, &state, &request)
            };
            Box::new(move |s: &mut ForeignService, bus| s.finished(job, result, bus))
        });
        if self.busy {
            InstallOutcome::Started(job)
        } else {
            InstallOutcome::Busy
        }
    }

    /// A job that cannot start: its end is published at once.
    fn fail_now(&mut self, name: &str, failure: Failure, bus: &Bus<'_, Self>) -> InstallOutcome {
        if !crate::desktop::package_name_ok(name) {
            return InstallOutcome::Invalid;
        }
        let job = self.job_id();
        bus.publish(&ultar_bus::messages::foreign::Progress {
            job,
            step: Step::Failed(failure),
            percent: 100,
            apps: Vec::new(),
        });
        InstallOutcome::Started(job)
    }

    fn finished(
        &mut self,
        job: u64,
        result: Result<Option<Vec<crate::state::AppRecord>>, Failure>,
        bus: &Bus<'_, Self>,
    ) {
        self.busy = false;
        self.runtime_size.set(None);
        let progress = |step, apps| ultar_bus::messages::foreign::Progress {
            job,
            step,
            percent: 100,
            apps,
        };
        match result {
            Ok(Some(apps)) => {
                let ids: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
                for a in apps {
                    self.state.put(a);
                }
                self.save(bus);
                bus.log(
                    Record::new(Level::Info, "foreign.installed").count("apps", ids.len() as u64),
                );
                bus.publish(&progress(Step::Done, ids));
            }
            Ok(None) => bus.publish(&progress(Step::Cancelled, Vec::new())),
            Err(failure) => {
                bus.log(Record::new(Level::Warn, "foreign.install_failed"));
                bus.publish(&progress(Step::Failed(failure), Vec::new()));
            }
        }
    }

    fn set_choices(&mut self, set: SetChoices, caller: String, bus: &Bus<'_, Self>) -> Outcome {
        if !set.choices.is_valid() {
            return Outcome::Invalid;
        }
        let Some(mut record) = self.state.get(&set.id).cloned() else {
            return Outcome::Unknown;
        };
        let widens = set.choices.widens(&record.choices());
        record.set_choices(&set.choices);
        if widens {
            // granting more is confirmed like an installation (never by the caller alone)
            if self.busy {
                return Outcome::Busy;
            }
            let job = self.job_id();
            self.busy = bus.later(move |client| {
                let result = jobs::change(client, &caller, job, &record);
                let apps = result.map(|r| r.map(|a| vec![a]));
                Box::new(move |s: &mut ForeignService, bus| s.finished(job, apps, bus))
            });
            return if self.busy {
                Outcome::Asking(job)
            } else {
                Outcome::Busy
            };
        }
        // taking back needs no confirmation
        if !jobs::register_one(bus.client(), &record) {
            return Outcome::Failed;
        }
        self.state.put(record);
        if self.save(bus) {
            Outcome::Done
        } else {
            Outcome::Failed
        }
    }

    fn remove(&mut self, remove: Remove, bus: &Bus<'_, Self>) -> Outcome {
        if self.busy {
            return Outcome::Busy;
        }
        let Some(record) = self.state.get(&remove.id).cloned() else {
            return Outcome::Unknown;
        };
        let call = ultar_bus::messages::launcher::Uninstall {
            app: record.id.clone(),
            keep_data: remove.keep_data,
        };
        let gone = bus.call(&call, std::time::Duration::from_secs(20));
        if gone.is_err() {
            return Outcome::Failed;
        }
        self.state.apps.retain(|a| a.id != record.id);
        if !record.icon.is_empty() {
            let _ = std::fs::remove_file(&record.icon);
        }
        self.save(bus);
        let deb = record.source == Source::Deb.word();
        if !deb {
            // a Windows program lives in its own data (gone with the launcher's uninstall)
            // and its installer here: nothing of it is in the Linux area
            crate::windows::forget(&self.ctx, &record);
        }
        if deb && !self.state.package_used(&record.package, &record.id) {
            let ctx = self.ctx.clone();
            self.busy = bus.later(move |_client: &Client| {
                let purged = jobs::purge(&ctx, &record.package);
                Box::new(
                    move |s: &mut ForeignService, bus: &Bus<'_, ForeignService>| {
                        s.busy = false;
                        s.runtime_size.set(None);
                        if purged.is_err() {
                            bus.log(Record::new(Level::Warn, "foreign.purge_failed"));
                        }
                    },
                )
            });
        }
        Outcome::Done
    }

    fn runtime_state(&self) -> RuntimeStateReply {
        let installed = self.ctx.layout.runtime_installed();
        RuntimeStateReply {
            installed,
            version: self.ctx.layout.runtime_version(),
            size_bytes: if installed {
                let size = self
                    .runtime_size
                    .get()
                    .unwrap_or_else(|| size_of(&self.ctx.layout.runtime(), 400_000));
                self.runtime_size.set(Some(size));
                size
            } else {
                0
            },
            busy: self.busy,
        }
    }
}

fn answer<C: Call>(request: IncomingRequest, reply: impl FnOnce(C) -> C::Reply) {
    let _ = match request.decode::<C>() {
        Ok(call) => request.reply(&reply(call)),
        Err(_) => request.fail(ErrorCode::BadPayload),
    };
}

impl Service for ForeignService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        if self.ctx.layout.ensure().is_err() {
            bus.log(Record::new(Level::Error, "foreign.root_not_usable"));
        }
        // an area replaced before the last start is emptied now, before any app of it runs
        let root = &self.ctx.layout.root;
        if ["linux.old", "linux.new"].iter().any(|n| root.join(n).exists()) {
            let ctx = self.ctx.clone();
            self.busy = bus.later(move |_client: &Client| {
                crate::component::tidy(&ctx);
                Box::new(|s: &mut ForeignService, _: &Bus<'_, ForeignService>| s.busy = false)
            });
        }
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let caller = request.caller().to_owned();
        match request.method() {
            m if m == Inspect::METHOD => {
                // reading a package runs a cell: on a worker, the loop stays free
                let job = self.job_id();
                let ctx = self.ctx.clone();
                let started = bus.later(move |_client: &Client| {
                    answer::<Inspect>(request, |i| InspectReply {
                        outcome: jobs::inspect(&ctx, &i.path, job),
                    });
                    Box::new(|_: &mut ForeignService, _: &Bus<'_, ForeignService>| {})
                });
                if !started {
                    bus.log(Record::new(Level::Warn, "foreign.inspect_refused_busy"));
                }
            }
            m if m == Install::METHOD => {
                answer::<Install>(request, |i| InstallReply {
                    outcome: self.install(i, caller, bus),
                });
            }
            m if m == List::METHOD => {
                let apps = self
                    .state
                    .apps
                    .iter()
                    .take(MAX_APPS)
                    .map(|a| a.listed())
                    .collect();
                answer::<List>(request, |_| ListReply { apps });
            }
            m if m == SetChoices::METHOD => {
                answer::<SetChoices>(request, |s| OutcomeReply {
                    outcome: self.set_choices(s, caller, bus),
                });
            }
            m if m == Remove::METHOD => {
                answer::<Remove>(request, |r| OutcomeReply {
                    outcome: self.remove(r, bus),
                });
            }
            m if m == RuntimeState::METHOD => {
                let reply = self.runtime_state();
                answer::<RuntimeState>(request, |_| reply);
            }
            m if m == RuntimeUpdate::METHOD => {
                // a signed component on a drive or in the areas (the Store's downloads);
                // without one, the area's sources would need the network (see `install`)
                let outcome = self.runtime_update(caller, bus);
                answer::<RuntimeUpdate>(request, |_| InstallReply { outcome });
            }
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
            }
        }
    }
}
