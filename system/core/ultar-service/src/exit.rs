//! Exit codes every Ultar daemon uses, so that the supervisor and the
//! tests can tell the reasons apart.

use std::process::ExitCode;

/// Clean stop.
pub const OK: i32 = 0;
/// The service failed while running or could not start its own work.
pub const FAILURE: i32 = 1;
/// The command line was not understood.
pub const USAGE: i32 = 2;
/// The service panicked (the panic hook logged it).
pub const PANIC: i32 = 70;
/// Hardening or signal setup failed; nothing was started.
pub const HARDENING: i32 = 71;
/// The service did not stop within its deadline after a stop signal.
pub const DEADLINE: i32 = 72;
/// A second stop signal arrived while the service was stopping.
pub const FORCED: i32 = 73;

/// Converts a code of this module for `main`.
pub fn code(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct_and_fit_a_byte() {
        let all = [OK, FAILURE, USAGE, PANIC, HARDENING, DEADLINE, FORCED];
        let unique: std::collections::BTreeSet<i32> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len());
        assert!(all.iter().all(|code| (0..=255).contains(code)));
        assert_eq!(code(OK), ExitCode::SUCCESS);
        let _ = code(-5);
    }
}
