//! The messages that make the GuC run a context: register it, give it its scheduling policy,
//! enable it (which also submits what waits in its ring), submit again, disable, deregister.
//! All of them go through the command buffers ([`crate::ctb`]) without a response; the GuC
//! reports back with events. Follows Linux' `xe_guc_submit.c` and its ABI headers (MIT).

use crate::ads::Class;
use alloc::vec::Vec;

/// Action: make a context known.
pub const ACTION_REGISTER_CONTEXT: u32 = 0x4502;
/// Action: forget a context.
pub const ACTION_DEREGISTER_CONTEXT: u32 = 0x4503;
/// Action: run what is in the context's ring.
pub const ACTION_SCHED_CONTEXT: u32 = 0x1000;
/// Action: enable or disable scheduling of a context.
pub const ACTION_SCHED_CONTEXT_MODE_SET: u32 = 0x1001;
/// Action: set the scheduling policy of a context.
pub const ACTION_UPDATE_CONTEXT_POLICIES: u32 = 0x100b;

/// Event: scheduling of a context was enabled or disabled (payload: the context's id).
pub const EVENT_SCHED_CONTEXT_MODE_DONE: u32 = 0x1002;
/// Event: a context was forgotten (payload: the context's id).
pub const EVENT_DEREGISTER_CONTEXT_DONE: u32 = 0x4600;
/// Event: the GuC reset a context that hung (payload: the context's id).
pub const EVENT_CONTEXT_RESET: u32 = 0x1008;
/// Event: an engine failed (payload: class, instance, reason).
pub const EVENT_ENGINE_FAILURE: u32 = 0x1009;

/// Number of context ids the GuC knows.
pub const MAX_CONTEXTS: u32 = 65535;

const REGISTRATION_FLAG_KMD: u32 = 1 << 0;

const KLV_EXECUTION_QUANTUM: u32 = 0x2001;
const KLV_PREEMPTION_TIMEOUT: u32 = 0x2002;
const KLV_SCHEDULING_PRIORITY: u32 = 0x2003;
const KLV_SLPM_GT_FREQUENCY: u32 = 0x2005;

/// How urgent a context is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// Kernel work that must not wait.
    KernelHigh = 0,
    /// Urgent work of a program.
    High = 1,
    /// The kernel's ordinary work.
    KernelNormal = 2,
    /// A program's ordinary work.
    Normal = 3,
}

/// Scheduling policy of a context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Urgency.
    pub priority: Priority,
    /// How long the context may run before another one gets its turn, in µs.
    pub timeslice_us: u32,
    /// How long the context may take to give way before the GuC resets it, in µs.
    pub preempt_timeout_us: u32,
}

impl Policy {
    /// What Linux gives a context by default: 1 ms slices, 640 ms to give way.
    pub const DEFAULT: Policy = Policy {
        priority: Priority::KernelNormal,
        timeslice_us: 1_000,
        preempt_timeout_us: 640_000,
    };
}

/// Registers context `id` for the engine(s) `submit_mask` (bit n: logical instance n) of `class`
/// with its image's `descriptor` ([`crate::lrc::Context::descriptor`]).
pub fn register(id: u32, class: Class, submit_mask: u32, descriptor: u64) -> [u32; 12] {
    [
        ACTION_REGISTER_CONTEXT,
        REGISTRATION_FLAG_KMD,
        id,
        class as u32,
        submit_mask,
        // no work queue: one image, one engine
        0,
        0,
        0,
        0,
        0,
        descriptor as u32,
        (descriptor >> 32) as u32,
    ]
}

/// Sets the policy of context `id`.
pub fn policies(id: u32, p: Policy) -> Vec<u32> {
    let mut m = alloc::vec![ACTION_UPDATE_CONTEXT_POLICIES, id];
    for (key, value) in [
        (KLV_SCHEDULING_PRIORITY, p.priority as u32),
        (KLV_EXECUTION_QUANTUM, p.timeslice_us),
        (KLV_PREEMPTION_TIMEOUT, p.preempt_timeout_us),
        (KLV_SLPM_GT_FREQUENCY, 0),
    ] {
        m.push(key << 16 | 1);
        m.push(value);
    }
    m
}

/// Enables (and thereby submits) or disables context `id`; answered by
/// [`EVENT_SCHED_CONTEXT_MODE_DONE`].
pub fn schedule(id: u32, enable: bool) -> [u32; 3] {
    [ACTION_SCHED_CONTEXT_MODE_SET, id, enable as u32]
}

/// Tells the GuC that the ring of the enabled context `id` has new work.
pub fn submit(id: u32) -> [u32; 2] {
    [ACTION_SCHED_CONTEXT, id]
}

/// Deregisters the disabled context `id`; answered by [`EVENT_DEREGISTER_CONTEXT_DONE`].
pub fn deregister(id: u32) -> [u32; 2] {
    [ACTION_DEREGISTER_CONTEXT, id]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_has_the_twelve_words_linux_sends() {
        let m = register(5, Class::Blitter, 1, 0x1_0300_0019);
        assert_eq!(m, [0x4502, 1, 5, 3, 1, 0, 0, 0, 0, 0, 0x0300_0019, 1]);
    }

    #[test]
    fn policies_are_four_keys_of_one_word() {
        let m = policies(5, Policy::DEFAULT);
        assert_eq!(m.len(), 10);
        assert_eq!(&m[..4], &[0x100b, 5, 0x2003_0001, 2]);
        assert_eq!(&m[4..8], &[0x2001_0001, 1_000, 0x2002_0001, 640_000]);
        assert_eq!(&m[8..], &[0x2005_0001, 0]);
    }

    #[test]
    fn schedule_submit_deregister() {
        assert_eq!(schedule(7, true), [0x1001, 7, 1]);
        assert_eq!(schedule(7, false), [0x1001, 7, 0]);
        assert_eq!(submit(7), [0x1000, 7]);
        assert_eq!(deregister(7), [0x4503, 7]);
    }
}
