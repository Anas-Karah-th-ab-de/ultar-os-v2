//! Transportschicht zum Shannon-Modem über die modem_v1-Zeichengeräte.
//!
//! Der Kernel rahmt/entrahmt SIPC5 je Knoten (iod,id). Userspace schreibt/liest die ROHEN
//! IPC-Nutzdaten. `UmtsLink` bildet die Kanal-IDs auf die passenden /dev-Knoten ab und bietet
//! nichtblockierendes Senden/Empfangen mit Timeout. Darüber setzt RIL-2 (SIM/Netz) und RIL-3 (Daten) auf.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;

// ---- Kanal-IDs (aus a30k modem_v1.h / modem-s327ap-sipc-pdata.dtsi) ----
pub const CH_FMT_0: u8 = 235; // /dev/umts_ipc0  – RIL-Steuerkanal (SIM/Netz)
pub const CH_FMT_1: u8 = 236; // /dev/umts_ipc1
pub const CH_RFS_0: u8 = 245; // /dev/umts_rfs0  – Remote File System
pub const CH_ROUTER: u8 = 25; // /dev/umts_router
pub const CH_PDP_0: u8 = 10; // Datenkanäle 10..24 (laufen als rmnet-Netzgeräte, nicht als Zeichenknoten)
pub const CH_PDP_MAX: u8 = 24;

/// Ein IPC-Rahmen aus Sicht des Transports: Kanal + rohe Nutzdaten (ohne SIPC5-Header).
#[derive(Clone, Debug, PartialEq)]
pub struct SipcFrame {
    pub channel: u8,
    pub payload: Vec<u8>,
}

/// Transport-Schnittstelle. RIL-2/RIL-3 programmieren GEGEN dieses Trait (mit Mock testbar).
pub trait ModemLink {
    /// Rohe IPC-Nutzdaten auf einem Kanal senden (Kernel rahmt SIPC5).
    fn send(&mut self, channel: u8, payload: &[u8]) -> std::io::Result<()>;
    /// Nächsten verfügbaren Rahmen holen; `None` bei Timeout. Nichtblockierend (poll + Timeout).
    fn recv(&mut self, timeout_ms: u32) -> std::io::Result<Option<SipcFrame>>;
}

/// Kanal-ID -> /dev-Knotenpfad. Nur Zeichenknoten (FMT/RFS/Router). PDP läuft über rmnet.
pub fn channel_to_node(channel: u8) -> Option<&'static str> {
    match channel {
        CH_FMT_0 => Some("/dev/umts_ipc0"),
        CH_FMT_1 => Some("/dev/umts_ipc1"),
        CH_RFS_0 => Some("/dev/umts_rfs0"),
        CH_ROUTER => Some("/dev/umts_router"),
        _ => None,
    }
}

const O_NONBLOCK: i32 = 0x800;
const POLLIN: i16 = 0x0001;

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

extern "C" {
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
}

/// Transport über die umts_*-Zeichengeräte.
pub struct UmtsLink {
    nodes: HashMap<u8, File>,
    /// Puffergröße je Lesevorgang (ein IPC-Frame pro read; dl_buffer_size=4096 laut pdata).
    bufsize: usize,
}

impl UmtsLink {
    /// Öffnet den FMT-Hauptkanal (/dev/umts_ipc0). Weitere Kanäle werden bei Bedarf geöffnet.
    pub fn open() -> std::io::Result<Self> {
        let mut l = UmtsLink {
            nodes: HashMap::new(),
            bufsize: 4096,
        };
        l.ensure(CH_FMT_0)?;
        Ok(l)
    }

    /// Leerer Transport ohne offene Knoten (zum gezielten Öffnen einzelner Kanäle).
    pub fn empty() -> Self {
        UmtsLink {
            nodes: HashMap::new(),
            bufsize: 4096,
        }
    }

    /// Stellt sicher, dass der Knoten für `channel` offen ist (nichtblockierend).
    pub fn ensure(&mut self, channel: u8) -> std::io::Result<()> {
        if self.nodes.contains_key(&channel) {
            return Ok(());
        }
        let path = channel_to_node(channel).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Kanal hat keinen Zeichenknoten (PDP läuft über rmnet)",
            )
        })?;
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_NONBLOCK)
            .open(path)?;
        self.nodes.insert(channel, f);
        Ok(())
    }
}

impl ModemLink for UmtsLink {
    fn send(&mut self, channel: u8, payload: &[u8]) -> std::io::Result<()> {
        self.ensure(channel)?;
        let f = self.nodes.get_mut(&channel).unwrap();
        f.write_all(payload) // Kernel baut den SIPC5-Header anhand der iod,id des Knotens
    }

    fn recv(&mut self, timeout_ms: u32) -> std::io::Result<Option<SipcFrame>> {
        if self.nodes.is_empty() {
            return Ok(None);
        }
        // poll über alle offenen Knoten
        let chans: Vec<u8> = self.nodes.keys().copied().collect();
        let mut pfds: Vec<PollFd> = chans
            .iter()
            .map(|c| PollFd {
                fd: self.nodes[c].as_raw_fd(),
                events: POLLIN,
                revents: 0,
            })
            .collect();
        let n = unsafe { poll(pfds.as_mut_ptr(), pfds.len() as u64, timeout_ms as i32) };
        if n <= 0 {
            return Ok(None); // Timeout (0) oder Fehler (-1) -> kein Frame
        }
        for (i, pfd) in pfds.iter().enumerate() {
            if pfd.revents & POLLIN != 0 {
                let ch = chans[i];
                let f = self.nodes.get_mut(&ch).unwrap();
                let mut buf = vec![0u8; self.bufsize];
                match f.read(&mut buf) {
                    Ok(0) => continue,
                    Ok(len) => {
                        buf.truncate(len);
                        return Ok(Some(SipcFrame {
                            channel: ch,
                            payload: buf,
                        }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(None)
    }
}

/// Mock-Transport für Tests von RIL-2/RIL-3 ohne Gerät: vorgegebene Antworten je gesendetem Frame.
pub struct MockLink {
    pub sent: Vec<SipcFrame>,
    pub inbox: std::collections::VecDeque<SipcFrame>,
}

impl MockLink {
    pub fn new() -> Self {
        MockLink {
            sent: Vec::new(),
            inbox: std::collections::VecDeque::new(),
        }
    }
    pub fn push_rx(&mut self, channel: u8, payload: Vec<u8>) {
        self.inbox.push_back(SipcFrame { channel, payload });
    }
}

impl Default for MockLink {
    fn default() -> Self {
        Self::new()
    }
}

impl ModemLink for MockLink {
    fn send(&mut self, channel: u8, payload: &[u8]) -> std::io::Result<()> {
        self.sent.push(SipcFrame {
            channel,
            payload: payload.to_vec(),
        });
        Ok(())
    }
    fn recv(&mut self, _timeout_ms: u32) -> std::io::Result<Option<SipcFrame>> {
        Ok(self.inbox.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_mapping() {
        assert_eq!(channel_to_node(CH_FMT_0), Some("/dev/umts_ipc0"));
        assert_eq!(channel_to_node(CH_RFS_0), Some("/dev/umts_rfs0"));
        assert_eq!(channel_to_node(12), None); // PDP hat keinen Zeichenknoten
    }

    #[test]
    fn mock_roundtrip() {
        let mut m = MockLink::new();
        m.send(CH_FMT_0, &[1, 2, 3]).unwrap();
        assert_eq!(m.sent[0].channel, CH_FMT_0);
        m.push_rx(CH_FMT_0, vec![9, 9]);
        let f = m.recv(0).unwrap().unwrap();
        assert_eq!(f.payload, vec![9, 9]);
    }
}
