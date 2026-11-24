//! Generic netlink: a family's id and its multicast groups by name
//! (`CTRL_CMD_GETFAMILY`).

use crate::attr::{self, Builder};
use crate::socket::{Socket, CHANGE};
use crate::NetError;

/// Family id of the controller.
pub const GENL_ID_CTRL: u16 = 16;
const CTRL_CMD_GETFAMILY: u8 = 3;
const CTRL_ATTR_FAMILY_ID: u16 = 1;
const CTRL_ATTR_FAMILY_NAME: u16 = 2;
const CTRL_ATTR_MCAST_GROUPS: u16 = 7;
const CTRL_ATTR_MCAST_GRP_NAME: u16 = 1;
const CTRL_ATTR_MCAST_GRP_ID: u16 = 2;

/// A generic netlink family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    /// Its id (the message type of its messages).
    pub id: u16,
    /// Multicast groups (name, id).
    pub groups: Vec<(String, u32)>,
}

impl Family {
    /// The id of a group.
    pub fn group(&self, name: &str) -> Option<u32> {
        self.groups
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }
}

/// A generic netlink request header (`genlmsghdr`).
pub fn header(family: u16, cmd: u8, flags: u16) -> Builder {
    Builder::new(family, flags).raw(&[cmd, 1, 0, 0])
}

/// The attributes of a generic netlink message (after `genlmsghdr`), with
/// its command.
pub fn split(payload: &[u8]) -> Result<(u8, &[u8]), NetError> {
    if payload.len() < 4 {
        return Err(NetError::Malformed);
    }
    Ok((payload[0], &payload[4..]))
}

/// Parses the controller's answer about a family.
pub fn parse_family(payload: &[u8]) -> Result<Family, NetError> {
    let (_, attrs) = split(payload)?;
    let id = attr::get_u16(attrs, CTRL_ATTR_FAMILY_ID).ok_or(NetError::Malformed)?;
    let mut groups = Vec::new();
    if let Some(list) = attr::find(attrs, CTRL_ATTR_MCAST_GROUPS) {
        for (_, g) in attr::attrs(list).take(32) {
            if let (Some(name), Some(gid)) = (
                attr::get_string(g, CTRL_ATTR_MCAST_GRP_NAME),
                attr::get_u32(g, CTRL_ATTR_MCAST_GRP_ID),
            ) {
                groups.push((name, gid));
            }
        }
    }
    Ok(Family { id, groups })
}

/// Looks a family up by name; [`NetError::Unavailable`] when the kernel
/// does not have it.
pub fn resolve(socket: &mut Socket, name: &str) -> Result<Family, NetError> {
    let request =
        header(GENL_ID_CTRL, CTRL_CMD_GETFAMILY, CHANGE).string(CTRL_ATTR_FAMILY_NAME, name);
    match socket.request(request) {
        Ok(answers) => parse_family(answers.first().ok_or(NetError::Malformed)?),
        Err(NetError::Kernel(e)) if e == libc::ENOENT => Err(NetError::Unavailable),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::socket::GENERIC;

    #[test]
    fn the_controller_knows_itself_and_not_nonsense() {
        let mut s = Socket::open(GENERIC).unwrap();
        let ctrl = resolve(&mut s, "nlctrl").unwrap();
        assert_eq!(ctrl.id, GENL_ID_CTRL);
        assert!(ctrl.group("notify").is_some());
        assert_eq!(
            resolve(&mut s, "no-such-family"),
            Err(NetError::Unavailable)
        );
        assert_eq!(split(&[1]), Err(NetError::Malformed));
        assert!(parse_family(&[3, 1, 0, 0]).is_err());
    }
}
