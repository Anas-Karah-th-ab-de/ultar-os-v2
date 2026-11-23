//! Shared rig for the service tests: a device with a service core on it.
#![allow(dead_code)]

use super::*;
use std::sync::{Arc, Mutex};
use ultar_update::contract::{Phase, Progress};
use ultar_update::engine::{Engine, Environment};
use ultar_update::error::UpdateError;
use ultar_update::fault::CancelFlag;
use ultar_update::filectl::FileBootControl;
use ultar_update::health::FixedSpace;
use ultar_update::service::{Access, Core, Job, Publish, SourceFactory};
use ultar_update::source::UpdateSource;

pub const ADMIN: &str = "system.settings";
pub const USER: &str = "app.files";
pub const NET: &str = "system.net";
pub const WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Serves whatever bundle is currently "published".
pub struct Published(pub Arc<Mutex<Option<Vec<u8>>>>);

impl SourceFactory for Published {
    fn open(&self) -> Result<Box<dyn UpdateSource + Send>, UpdateError> {
        let bundle = self.0.lock().unwrap().clone();
        let mut source = MemSource::new(bundle.as_deref().unwrap_or_default());
        source.empty = bundle.is_none();
        Ok(Box::new(source))
    }
}

#[derive(Default)]
pub struct Collect(pub Mutex<Vec<Progress>>);

impl Publish for Collect {
    fn publish(&self, progress: &Progress) {
        self.0.lock().unwrap().push(progress.clone());
    }
}

pub struct Rig {
    pub device: Device,
    pub core: Arc<Core<FileBootControl>>,
    pub cancel: Arc<CancelFlag>,
    pub published: Arc<Mutex<Option<Vec<u8>>>>,
    pub events: Arc<Collect>,
}

pub fn core_on(
    device: &Device,
    sources: Arc<dyn SourceFactory>,
    publisher: Arc<dyn Publish>,
) -> (Arc<Core<FileBootControl>>, Arc<CancelFlag>) {
    let cancel = Arc::new(CancelFlag::new());
    let env = Environment {
        clock: device.clock.clone(),
        space: Arc::new(FixedSpace(u64::MAX)),
        control: cancel.clone(),
    };
    let engine = Engine::open(device.config.clone(), env, device.boot.clone()).unwrap();
    let access = Access {
        admins: vec![ADMIN.to_string()],
        network_identity: Some(NET.to_string()),
        offline_roots: vec![device.dir.path().join("usb")],
    };
    std::fs::create_dir_all(device.dir.path().join("usb")).unwrap();
    (
        Arc::new(Core::new(
            engine,
            cancel.clone(),
            sources,
            publisher,
            access,
        )),
        cancel,
    )
}

pub fn rig() -> Rig {
    let device = Device::new();
    let published = Arc::new(Mutex::new(None));
    let events = Arc::new(Collect::default());
    let (core, cancel) = core_on(
        &device,
        Arc::new(Published(published.clone())),
        events.clone(),
    );
    Rig {
        device,
        core,
        cancel,
        published,
        events,
    }
}

impl Rig {
    pub fn publish(&self, release: &Release) {
        *self.published.lock().unwrap() = Some(release.bundle.clone());
    }

    pub fn job(&self, caller: &str, job: Job) -> Result<(), UpdateError> {
        self.core.accept(caller, &job)?;
        self.core.run(job)
    }

    pub fn phases(&self) -> Vec<Phase> {
        let mut phases: Vec<Phase> = self
            .events
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|p| p.phase)
            .collect();
        phases.dedup();
        phases
    }
}
