//! Binding memory into an address space (`DRM_IOCTL_XE_VM_BIND`): buffers, ranges that map to
//! nothing ("null", `DRM_XE_VM_BIND_FLAG_NULL`), removing, and replacing what a new binding
//! covers – a binding that is only partly covered keeps its other parts, as Linux splits it.

use super::*;

/// `DRM_XE_VM_BIND_FLAG_NULL`: the range maps to nothing.
const BIND_FLAG_NULL: u32 = 1 << 2;

impl<B: Backend> File<B> {
    /// One bind operation (80 bytes of `struct drm_xe_vm_bind_op`).
    fn bind_op(&mut self, be: &mut B, vm_id: u32, op: &[u8]) -> Result<(), Errno> {
        let b = fields(op, 80, 52, true)?;
        let (handle, pat, offset) = (u32_at(b, 8), u16_at(b, 12), u64_at(b, 16));
        let (range, addr, kind, flags) =
            (u64_at(b, 24), u64_at(b, 32), u32_at(b, 40), u32_at(b, 44));
        // flags: read-only, immediate and dumpable change nothing here; null maps to nothing;
        // the others are not there
        if u16_at(b, 14) != 0 || flags & !0b1111 != 0 || pat > 4 {
            return Err(Errno::Inval);
        }
        let null = flags & BIND_FLAG_NULL != 0;
        match kind {
            0 if null => {
                let aligned = (range | addr) % PAGE == 0 && range != 0;
                let fits = addr.checked_add(range).is_some_and(|e| e <= VA_END);
                if !aligned || !fits || handle != 0 || offset != 0 {
                    return Err(Errno::Inval);
                }
                self.unbind(be, vm_id, addr, range)?;
                let vm = self.vms.get_mut(&vm_id).ok_or(Errno::NoEnt)?;
                if vm.binds.len() >= MAX_BINDS {
                    return Err(Errno::NoMem);
                }
                be.vm_map_null(vm.backend, addr, range / PAGE)?;
                vm.binds.push(Bind { va: addr, pages: range / PAGE, handle: 0, buffer: None });
                Ok(())
            }
            0 => {
                let (buffer, size, owner) = self.buffers.get(&handle).ok_or(Errno::NoEnt)?;
                let aligned = (range | addr | offset) % PAGE == 0 && range != 0;
                let fits = offset.checked_add(range).is_some_and(|e| e <= *size)
                    && addr.checked_add(range).is_some_and(|e| e <= VA_END);
                if !aligned || !fits || (*owner != 0 && *owner != vm_id) {
                    return Err(Errno::Inval);
                }
                let buffer = buffer.clone();
                let first = (offset / PAGE) as usize;
                let all = be.buffer_pages(&buffer);
                let pages = all
                    .get(first..first + (range / PAGE) as usize)
                    .ok_or(Errno::Inval)?;
                self.unbind(be, vm_id, addr, range)?;
                let vm = self.vms.get_mut(&vm_id).ok_or(Errno::NoEnt)?;
                if vm.binds.len() >= MAX_BINDS {
                    return Err(Errno::NoMem);
                }
                be.vm_map(vm.backend, addr, pages)?;
                vm.binds.push(Bind {
                    va: addr,
                    pages: range / PAGE,
                    handle,
                    buffer: Some(buffer),
                });
                Ok(())
            }
            1 => {
                if (range | addr) % PAGE != 0 || range == 0 || handle != 0 || null {
                    return Err(Errno::Inval);
                }
                self.unbind(be, vm_id, addr, range)
            }
            3 if !null => {
                let vm = self.vms.get(&vm_id).ok_or(Errno::NoEnt)?;
                let ranges: Vec<(u64, u64)> = vm
                    .binds
                    .iter()
                    .filter(|x| x.handle == handle)
                    .map(|x| (x.va, x.pages * PAGE))
                    .collect();
                ranges
                    .into_iter()
                    .try_for_each(|(va, len)| self.unbind(be, vm_id, va, len))
            }
            _ => Err(Errno::Inval),
        }
    }

    /// Removes what is bound inside `[addr, addr + range)`. A binding that only partly lies
    /// inside keeps its parts outside (as Linux splits a mapping).
    fn unbind(&mut self, be: &mut B, vm_id: u32, addr: u64, range: u64) -> Result<(), Errno> {
        let vm = self.vms.get_mut(&vm_id).ok_or(Errno::NoEnt)?;
        let end = addr + range;
        let touches = |x: &Bind<B>| x.va < end && x.va + x.pages * PAGE > addr;
        if vm.binds.len() + 2 * vm.binds.iter().filter(|x| touches(x)).count() > MAX_BINDS + 2 {
            return Err(Errno::NoMem);
        }
        let (hit, mut keep): (Vec<_>, Vec<_>) = core::mem::take(&mut vm.binds).into_iter().partition(touches);
        let mut result = Ok(());
        for x in hit {
            let (from, to) = (x.va.max(addr), (x.va + x.pages * PAGE).min(end));
            if result.is_ok() {
                result = be.vm_unmap(vm.backend, from, (to - from) / PAGE);
            }
            if result.is_err() {
                keep.push(x);
                continue;
            }
            let part = |va: u64, stop: u64| Bind { va, pages: (stop - va) / PAGE, handle: x.handle, buffer: x.buffer.clone() };
            if x.va < from {
                keep.push(part(x.va, from));
            }
            if to < x.va + x.pages * PAGE {
                keep.push(part(to, x.va + x.pages * PAGE));
            }
        }
        vm.binds = keep;
        result
    }

    /// `DRM_IOCTL_XE_VM_BIND`. The operations take effect before this returns, so the fences
    /// are stored right away.
    pub fn vm_bind(&mut self, be: &mut B, arg: &[u8], user: &mut impl User) -> Result<(), Errno> {
        let b = fields(arg, 136, 120, true)?;
        let (vm_id, queue, count) = (u32_at(b, 8), u32_at(b, 12), u32_at(b, 20));
        if u32_at(b, 16) != 0 || u32_at(b, 104) != 0 {
            return Err(Errno::Inval);
        }
        // a bind queue of this address space may be named; binds take effect at once anyway
        let named = |q: &Queue| q.backend.is_none() && q.vm == vm_id;
        if queue != 0 && !self.queues.get(&queue).is_some_and(named) {
            return Err(Errno::Inval);
        }
        if count == 0 || count > MAX_BIND_OPS {
            return Err(Errno::Inval);
        }
        if !self.vms.contains_key(&vm_id) {
            return Err(Errno::NoEnt);
        }
        let syncs = sync::read(user, u64_at(b, 112), u32_at(b, 108))?;
        self.syncs_ready(be, &syncs)?;
        if count == 1 {
            self.bind_op(be, vm_id, &b[24..104])?;
        } else {
            let vector = u64_at(b, 24);
            for i in 0..count as u64 {
                let op = user.read(vector + i * 80, 80).ok_or(Errno::Fault)?;
                self.bind_op(be, vm_id, &op)?;
            }
        }
        self.finish_now(&syncs, user)
    }
}
