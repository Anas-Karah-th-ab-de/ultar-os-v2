//! `wl_shm`, `wl_shm_pool`, `wl_buffer`: the app's pixels in memory files.
//! The bridge never maps them; it reads rows with `pread` on commit.

use crate::limits::{MAX_BUFFERS, MAX_POOLS, MAX_POOL_BYTES, MAX_SIDE};
use crate::pixels::Layout;
use crate::state::{refuse, Buffer, Pool, State, IMPLEMENTATION, NO_MEMORY};
use std::sync::Arc;
use wayland_server::backend::ClientId;
use wayland_server::protocol::{
    wl_buffer, wl_buffer::WlBuffer, wl_shm, wl_shm::WlShm, wl_shm_pool, wl_shm_pool::WlShmPool,
};
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

impl GlobalDispatch<WlShm, ()> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlShm>,
        _: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        let shm = data_init.init(resource, ());
        shm.format(wl_shm::Format::Argb8888);
        shm.format(wl_shm::Format::Xrgb8888);
    }
}

impl Dispatch<WlShm, ()> for State {
    fn request(
        state: &mut State,
        client: &Client,
        shm: &WlShm,
        request: wl_shm::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let wl_shm::Request::CreatePool { id, fd, size } = request else {
            return;
        };
        if size <= 0 || size as u64 > MAX_POOL_BYTES {
            shm.post_error(wl_shm::Error::InvalidStride, "pool size out of range");
            return;
        }
        if state.counts(&client.id()).pools >= MAX_POOLS {
            refuse(&state.dh, client, NO_MEMORY, "too many pools");
            return;
        }
        let pool = data_init.init(id, ());
        state.counts(&client.id()).pools += 1;
        state.objects.pools.insert(
            pool.id(),
            Pool {
                fd: Arc::new(fd),
                size: size as u64,
            },
        );
    }
}

/// A buffer as the client asked for it.
struct Asked {
    offset: i32,
    width: i32,
    height: i32,
    stride: i32,
    format: WEnum<wl_shm::Format>,
}

impl Asked {
    /// Whether it lies inside a pool of `pool_size` bytes.
    fn fits(&self, pool_size: u64) -> bool {
        let (w, h) = (self.width, self.height);
        let end = i64::from(self.offset) + i64::from(self.stride) * i64::from(h);
        (1..=MAX_SIDE).contains(&w)
            && (1..=MAX_SIDE).contains(&h)
            && self.offset >= 0
            && i64::from(self.stride) >= i64::from(w) * 4
            && end <= pool_size as i64
    }
}

fn create_buffer(
    state: &mut State,
    client: &Client,
    pool: &WlShmPool,
    id: New<WlBuffer>,
    a: &Asked,
    data_init: &mut DataInit<'_, State>,
) {
    let Some((fd, pool_size)) = state
        .objects
        .pools
        .get(&pool.id())
        .map(|p| (p.fd.clone(), p.size))
    else {
        refuse(&state.dh, client, IMPLEMENTATION, "unknown pool");
        return;
    };
    let alpha = match a.format {
        WEnum::Value(wl_shm::Format::Argb8888) => true,
        WEnum::Value(wl_shm::Format::Xrgb8888) => false,
        _ => {
            pool.post_error(wl_shm::Error::InvalidFormat, "only ARGB8888 and XRGB8888");
            return;
        }
    };
    if !a.fits(pool_size) {
        pool.post_error(wl_shm::Error::InvalidStride, "buffer outside the pool");
        return;
    }
    if state.counts(&client.id()).buffers >= MAX_BUFFERS {
        refuse(&state.dh, client, NO_MEMORY, "too many buffers");
        return;
    }
    let layout = Layout {
        offset: a.offset as u64,
        width: a.width as u32,
        height: a.height as u32,
        stride: a.stride as u32,
        alpha,
    };
    let buffer = data_init.init(id, ());
    state.counts(&client.id()).buffers += 1;
    state
        .objects
        .buffers
        .insert(buffer.id(), Buffer { fd, layout });
}

impl Dispatch<WlShmPool, ()> for State {
    fn request(
        state: &mut State,
        client: &Client,
        pool: &WlShmPool,
        request: wl_shm_pool::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        match request {
            wl_shm_pool::Request::CreateBuffer {
                id,
                offset,
                width,
                height,
                stride,
                format,
            } => {
                let asked = Asked {
                    offset,
                    width,
                    height,
                    stride,
                    format,
                };
                create_buffer(state, client, pool, id, &asked, data_init);
            }
            wl_shm_pool::Request::Resize { size } => {
                let Some(p) = state.objects.pools.get_mut(&pool.id()) else {
                    return;
                };
                if size < p.size as i32 || size as u64 > MAX_POOL_BYTES {
                    pool.post_error(
                        wl_shm::Error::InvalidStride,
                        "pool may only grow, within the limit",
                    );
                    return;
                }
                p.size = size as u64;
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut State, client: ClientId, resource: &WlShmPool, _: &()) {
        if state.objects.pools.remove(&resource.id()).is_some() {
            let c = state.counts(&client);
            c.pools = c.pools.saturating_sub(1);
        }
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &WlBuffer,
        _: wl_buffer::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }

    fn destroyed(state: &mut State, client: ClientId, resource: &WlBuffer, _: &()) {
        if state.objects.buffers.remove(&resource.id()).is_some() {
            let c = state.counts(&client);
            c.buffers = c.buffers.saturating_sub(1);
        }
    }
}
