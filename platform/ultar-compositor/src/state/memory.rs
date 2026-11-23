//! Pixel memory: per connection, per uid, in total, and the reserve of
//! the system clients (a lock screen always gets its buffers).

use super::{is_system, ClientId, State};

impl State {
    /// Pixel memory of all clients together.
    pub fn total_bytes(&self) -> u64 {
        self.clients.values().map(|c| c.bytes).sum()
    }

    /// Pixel memory of the clients of one uid.
    pub fn uid_bytes(&self, uid: u32) -> u64 {
        self.clients
            .values()
            .filter(|c| c.conn.peer.uid == uid)
            .map(|c| c.bytes)
            .sum()
    }

    /// Bytes `client` may still add: the least of its own room, its uid's
    /// room (system uids excepted) and the room of all clients (without
    /// the reserve for non-system clients).
    pub fn memory_room(&self, client: ClientId) -> u64 {
        let limits = self.config.limits;
        let share = limits.share;
        let Some(c) = self.clients.get(&client) else {
            return 0;
        };
        let own = limits.client_bytes.saturating_sub(c.bytes);
        let used = self.total_bytes();
        if is_system(c.rights) {
            return own.min(share.total_bytes.saturating_sub(used));
        }
        let open = share.total_bytes.saturating_sub(share.reserved_bytes);
        let uid = share
            .uid_bytes
            .saturating_sub(self.uid_bytes(c.conn.peer.uid));
        own.min(uid).min(open.saturating_sub(used))
    }
}
