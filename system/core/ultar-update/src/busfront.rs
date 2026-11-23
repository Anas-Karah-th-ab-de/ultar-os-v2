//! Puts the update service on the bus: request dispatch, the worker for long operations, and
//! the progress topic.

use crate::bootctl::BootControl;
use crate::contract::{
    Accepted, Apply, BootPrevious, Cancel, Check, Download, GetPolicy, History, HistoryReply,
    Network, Offline, PolicyMessage, Progress, Status,
};
use crate::error::UpdateError;
use crate::service::{Core, Job, Publish};
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use ultar_bus::{Call, Client, ErrorCode, IncomingRequest, ServiceEndpoint};

/// Publishes state changes on `update.progress`.
pub struct BusPublisher(pub Client);

impl Publish for BusPublisher {
    fn publish(&self, progress: &Progress) {
        // A lost progress event is not worth failing an update for; `status` has the truth.
        let _ = self.0.publish_unconfirmed(progress);
    }
}

/// The bus error code for a service failure.
pub fn error_code(error: &UpdateError) -> ErrorCode {
    match error {
        UpdateError::PermissionDenied => ErrorCode::PermissionDenied,
        other => ErrorCode::Service(other.code()),
    }
}

fn accept_job<B: BootControl, C: Call<Reply = Accepted>>(
    core: &Core<B>,
    request: IncomingRequest,
    jobs: &SyncSender<Job>,
    make: impl FnOnce(C) -> Job,
) {
    let caller = request.caller().to_string();
    let _ = request.respond::<C>(|call| {
        let job = make(call);
        core.accept(&caller, &job).map_err(|e| error_code(&e))?;
        jobs.try_send(job).map_err(|_| {
            core.release();
            error_code(&UpdateError::Busy)
        })?;
        Ok(Accepted {})
    });
}

fn answer<C: Call>(
    request: IncomingRequest,
    handler: impl FnOnce(C) -> Result<C::Reply, UpdateError>,
) {
    let _ = request.respond::<C>(|call| handler(call).map_err(|e| error_code(&e)));
}

/// Handles one request. Long operations are queued for the worker and acknowledged at once.
pub fn dispatch<B: BootControl>(core: &Core<B>, request: IncomingRequest, jobs: &SyncSender<Job>) {
    let caller = request.caller().to_string();
    let method = request.method().to_string();
    match method.as_str() {
        "status" => answer::<Status>(request, |_| core.status()),
        "check" => accept_job(core, request, jobs, |_: Check| Job::Check),
        "download" => accept_job(core, request, jobs, |d: Download| Job::Download {
            allow_metered: d.allow_metered,
        }),
        "apply" => accept_job(core, request, jobs, |_: Apply| Job::Apply),
        "offline" => accept_job(core, request, jobs, |o: Offline| Job::Offline {
            path: PathBuf::from(o.path),
            allow_downgrade: o.allow_downgrade,
        }),
        "cancel" => answer::<Cancel>(request, |_| core.cancel(&caller).map(|()| Accepted {})),
        "boot_previous" => answer::<BootPrevious>(request, |_| {
            core.boot_previous(&caller).map(|()| Accepted {})
        }),
        "network" => answer::<Network>(request, |n| {
            core.network(&caller, n.metered).map(|()| Accepted {})
        }),
        "history" => answer::<History>(request, |h| {
            Ok(HistoryReply {
                entries: core.history(h.limit),
            })
        }),
        "get_policy" => answer::<GetPolicy>(request, |_| Ok(core.policy())),
        "set_policy" => answer::<PolicyMessage>(request, |policy| {
            core.set_policy(&caller, &policy).map(|()| Accepted {})
        }),
        _ => {
            let _ = request.fail(ErrorCode::UnknownMethod);
        }
    }
}

/// Serves requests from `endpoint` until the bus connection ends. Long operations run on one
/// worker thread, one at a time; quick calls are answered on the calling thread meanwhile.
pub fn serve<B: BootControl + 'static>(core: Arc<Core<B>>, endpoint: ServiceEndpoint) {
    let (jobs, queue) = sync_channel::<Job>(1);
    let worker_core = core.clone();
    let worker = std::thread::Builder::new()
        .name("update-worker".into())
        .spawn(move || {
            for job in queue {
                let _ = worker_core.run(job);
            }
        });
    while let Ok(request) = endpoint.recv() {
        dispatch(&core, request, &jobs);
    }
    drop(jobs);
    if let Ok(handle) = worker {
        let _ = handle.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_on_the_bus() {
        assert_eq!(
            error_code(&UpdateError::PermissionDenied),
            ErrorCode::PermissionDenied
        );
        let busy = error_code(&UpdateError::Busy);
        assert_eq!(busy, ErrorCode::Service(UpdateError::Busy.code()));
        match busy {
            ErrorCode::Service(code) => {
                assert_eq!(UpdateError::key_of_code(code), Some("update.error.busy"));
            }
            other => panic!("{other:?}"),
        }
    }
}
