//! PID namespaces: which processes a process can see and under which numbers.
//!
//! The kernel keeps one global number per thread. A namespace maps its own (local) numbers to global
//! ones; a thread has a number in its own namespace and in every ancestor namespace, and is visible
//! exactly there. The initial namespace uses the global numbers directly.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

/// Deepest allowed nesting of pid namespaces below the initial one.
pub const MAX_LEVEL: u32 = 8;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static INITIAL: spin::Once<Arc<PidNs>> = spin::Once::new();

/// One pid namespace.
pub struct PidNs {
    id: u64,
    level: u32,
    parent: Option<Arc<PidNs>>,
    next: AtomicU64,
    init: AtomicU64,
    dead: AtomicBool,
    map: Mutex<Vec<(u64, u64)>>,
}

impl PidNs {
    /// The initial namespace (global numbers).
    pub fn initial() -> Arc<PidNs> {
        INITIAL
            .call_once(|| {
                // lives as long as the machine: it belongs to the kernel, not to whoever asks first
                crate::heap::kernel(|| {
                    Arc::new(PidNs {
                        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                        level: 0,
                        parent: None,
                        next: AtomicU64::new(1),
                        init: AtomicU64::new(0),
                        dead: AtomicBool::new(false),
                        map: Mutex::new(Vec::new()),
                    })
                })
            })
            .clone()
    }

    /// New empty namespace below `parent`; `None` when the nesting limit is reached.
    pub fn child(parent: &Arc<PidNs>) -> Option<Arc<PidNs>> {
        if parent.level >= MAX_LEVEL {
            return None;
        }
        Some(Arc::new(PidNs {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            level: parent.level + 1,
            parent: Some(parent.clone()),
            next: AtomicU64::new(1),
            init: AtomicU64::new(0),
            dead: AtomicBool::new(false),
            map: Mutex::new(Vec::new()),
        }))
    }

    /// Identity of the namespace (shown by `/proc/<pid>/ns/pid`).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Whether this is the initial namespace.
    pub fn is_initial(&self) -> bool {
        self.level == 0
    }

    /// Number of global thread `global` in this namespace; `None` if it is not visible here.
    pub fn local(&self, global: u64) -> Option<u64> {
        if self.level == 0 {
            return Some(global);
        }
        self.map.lock().iter().find(|e| e.1 == global).map(|e| e.0)
    }

    /// Global number of the thread known as `local` in this namespace.
    pub fn global(&self, local: u64) -> Option<u64> {
        if self.level == 0 {
            return Some(local);
        }
        self.map.lock().iter().find(|e| e.0 == local).map(|e| e.1)
    }

    /// Whether `other` is this namespace or one nested below it.
    pub fn contains(&self, other: &Arc<PidNs>) -> bool {
        let mut cur: Option<&Arc<PidNs>> = Some(other);
        while let Some(ns) = cur {
            if ns.id == self.id {
                return true;
            }
            cur = ns.parent.as_ref();
        }
        false
    }

    /// Global number of the namespace's first process (its pid 1); 0 in the initial namespace or
    /// before the first process exists.
    pub fn init_global(&self) -> u64 {
        self.init.load(Ordering::Relaxed)
    }

    /// Whether the namespace's pid 1 has ended (no new process may enter).
    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Relaxed)
    }

    /// Close the namespace after its pid 1 ended.
    pub fn mark_dead(&self) {
        self.dead.store(true, Ordering::Relaxed);
    }

    fn register(&self, global: u64) {
        if self.level == 0 {
            return;
        }
        let local = self.next.fetch_add(1, Ordering::Relaxed);
        if local == 1 {
            self.init.store(global, Ordering::Relaxed);
        }
        self.map.lock().push((local, global));
    }

    fn unregister(&self, global: u64) {
        if self.level != 0 {
            self.map.lock().retain(|e| e.1 != global);
        }
    }
}

/// Membership of one thread in its pid namespace and all ancestors. Dropping it frees the numbers.
pub struct PidLink {
    ns: Arc<PidNs>,
    global: u64,
}

impl PidLink {
    /// Give the thread with global number `global` a number in `ns` and in every ancestor.
    pub fn new(ns: Arc<PidNs>, global: u64) -> PidLink {
        let mut cur: Option<&Arc<PidNs>> = Some(&ns);
        while let Some(n) = cur {
            n.register(global);
            cur = n.parent.as_ref();
        }
        PidLink { ns, global }
    }

    /// Namespace the thread lives in.
    pub fn ns(&self) -> &Arc<PidNs> {
        &self.ns
    }
}

impl Drop for PidLink {
    fn drop(&mut self) {
        let mut cur: Option<&Arc<PidNs>> = Some(&self.ns);
        while let Some(n) = cur {
            n.unregister(self.global);
            cur = n.parent.as_ref();
        }
    }
}
