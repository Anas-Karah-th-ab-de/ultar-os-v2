//! Buffers whose size a caller chooses: allocated so that the request can be *refused* (limits of
//! the group, dynamic rules, kernel reserve – see `heap::fallible`) and the system call reports
//! `ENOMEM` instead of the kernel insisting on the memory.

use crate::heap::fallible;

/// A zeroed byte buffer of `n` bytes, or `None` if memory is refused (policy applies).
pub fn try_vec(n: usize) -> Option<alloc::vec::Vec<u8>> {
    let mut v = alloc::vec::Vec::new();
    fallible(|| v.try_reserve_exact(n)).ok()?;
    v.resize(n, 0);
    Some(v)
}

/// An empty vector with room for `n` elements, or `None` if memory is refused.
pub fn try_with_capacity<T>(n: usize) -> Option<alloc::vec::Vec<T>> {
    let mut v = alloc::vec::Vec::new();
    fallible(|| v.try_reserve_exact(n)).ok()?;
    Some(v)
}

/// Make room for `extra` more elements; `false` if memory is refused (the vector is unchanged).
pub fn try_reserve<T>(v: &mut alloc::vec::Vec<T>, extra: usize) -> bool {
    fallible(|| v.try_reserve(extra)).is_ok()
}

/// A copy of `s`, or `None` if memory is refused.
pub fn try_copy(s: &[u8]) -> Option<alloc::vec::Vec<u8>> {
    let mut v = try_with_capacity(s.len())?;
    v.extend_from_slice(s);
    Some(v)
}
