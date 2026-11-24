//! Several nodes wired together in memory, exactly as the network would.
#![allow(dead_code)]

pub mod net;

use std::path::PathBuf;
use std::time::Instant;
use ultar_bus::messages::mesh::DeviceKind;
use ultar_mesh::device::{default_capabilities, id_of};
use ultar_mesh::node::{Config, Effect, Node};
use ultar_mesh::trust::Trust;

pub struct World {
    pub nodes: Vec<Node>,
    pub seen: Vec<Vec<Effect>>,
    pub dir: PathBuf,
    pub now: Instant,
    /// Frames from node `.0` to node `.1` are lost while set.
    pub cut: Option<(usize, usize)>,
}

pub fn key(n: u8) -> [u8; 32] {
    [n + 1; 32]
}

impl World {
    pub fn new(tag: &str, kinds: &[(DeviceKind, &[&str])]) -> World {
        let dir = std::env::temp_dir().join(format!("mesh-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let nodes = kinds
            .iter()
            .enumerate()
            .map(|(i, (kind, apps))| {
                let config = Config {
                    name: format!("dev{i}"),
                    kind: *kind,
                    capabilities: default_capabilities(*kind),
                    apps: apps.iter().map(|a| (*a).to_owned()).collect(),
                    inbox: dir.join(format!("inbox{i}")),
                    share: vec![dir.clone()],
                    remote_view: false,
                };
                Node::new(config, &key(i as u8), Trust::in_memory())
            })
            .collect::<Vec<_>>();
        let seen = nodes.iter().map(|_| Vec::new()).collect();
        World {
            nodes,
            seen,
            dir,
            now: Instant::now(),
            cut: None,
        }
    }

    pub fn id(&self, i: usize) -> String {
        id_of(&key(i as u8))
    }

    fn index(&self, id: &str) -> usize {
        (0..self.nodes.len()).find(|i| self.id(*i) == id).unwrap()
    }

    /// Runs `effects` of node `from` until nothing is on the wire any more.
    pub fn run(&mut self, from: usize, effects: Vec<Effect>) {
        let mut queue: Vec<(usize, Effect)> = effects.into_iter().map(|e| (from, e)).collect();
        while !queue.is_empty() {
            let (at, effect) = queue.remove(0);
            match effect {
                Effect::Send(to, frame) => {
                    let to = self.index(&to);
                    if self.cut == Some((at, to)) {
                        continue;
                    }
                    let sender = self.id(at);
                    let more = self.nodes[to].frame(&sender, frame, self.now);
                    queue.extend(more.into_iter().map(|e| (to, e)));
                }
                other => self.seen[at].push(other),
            }
        }
    }

    pub fn link(&mut self, a: usize, b: usize) {
        let (ia, ib) = (self.id(a), self.id(b));
        let ea = self.nodes[a].link_up(&ib, &key(b as u8));
        let eb = self.nodes[b].link_up(&ia, &key(a as u8));
        self.run(a, ea);
        self.run(b, eb);
    }

    /// `b` shows a code, `a` types `typed` (`None`: the right one).
    pub fn pair(&mut self, a: usize, b: usize, typed: Option<&str>) {
        let code = self.nodes[b].pair_begin(self.now).code;
        let ib = self.id(b);
        let (ok, effects) = self.nodes[a].pair_join(&ib, typed.unwrap_or(&code));
        assert!(ok);
        self.run(a, effects);
    }

    pub fn paired(&self, a: usize, b: usize) -> bool {
        let ib = self.id(b);
        self.nodes[a]
            .state()
            .peers
            .iter()
            .any(|p| p.id == ib && p.paired)
    }

    pub fn take(&mut self, i: usize) -> Vec<Effect> {
        std::mem::take(&mut self.seen[i])
    }

    pub fn file(&self, name: &str, content: &[u8]) -> String {
        let path = self.dir.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_str().unwrap().to_owned()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
