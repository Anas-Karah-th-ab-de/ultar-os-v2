//! The frame budget: what an app can make the compositor draw is bounded,
//! so no app can make frames slow for everyone (the shell and the lock
//! screen included).
//!
//! Every surface of a uid that is not a system client (shell, lock
//! screen, input method, system dialogs) costs what it makes the renderer
//! touch beyond plain opaque copies: translucent content in full, and for
//! windows the title bar and shadow band too. Per uid at most
//! [`BUDGET_SCREENS`] screens of that, [`BUDGET_SCREENS_TOTAL`] for all
//! such uids together, and at most
//! [`crate::config::ShareLimits::windows_per_uid`] visible windows per
//! uid. The budget is spent from the top of the stack down, so what the
//! user sees on top is drawn and what is left out is below it; the
//! focused window is never left out. Left-out surfaces are neither drawn
//! nor hit.

use super::{is_system, Placed, State, SurfaceKey};
use std::collections::HashMap;
use ultar_surface::proto::{Rights, Role};

/// Screens of cost one uid may cause.
pub const BUDGET_SCREENS: i64 = 2;
/// Screens of cost all non-system uids together may cause.
pub const BUDGET_SCREENS_TOTAL: i64 = 3;

/// What has been spent while walking the scene from the top.
struct Ledger {
    per_uid: HashMap<u32, i64>,
    windows: HashMap<u32, usize>,
    remaining: i64,
    budget: i64,
    cap: usize,
}

impl State {
    /// Applies the budget to a scene (bottom to top); returns what stays
    /// and what is left out.
    pub(super) fn cull(&self, scene: Vec<Placed>) -> (Vec<Placed>, Vec<SurfaceKey>) {
        let screens: i64 = self.layout.enabled().map(|o| o.logical_rect().area()).sum();
        let mut ledger = Ledger {
            per_uid: HashMap::new(),
            windows: HashMap::new(),
            remaining: screens * BUDGET_SCREENS_TOTAL,
            budget: screens * BUDGET_SCREENS,
            cap: self.config.limits.share.windows_per_uid,
        };
        let parent_of = |p: &Placed| match self.surface(p.key).map(|s| s.role) {
            Some(Role::Popup { parent, .. }) => Some(SurfaceKey {
                client: p.key.client,
                surface: parent,
            }),
            _ => None,
        };
        let mut keep = vec![true; scene.len()];
        // Windows and layers first, from the top; then popups whose parent
        // stays (a popup never shows without its window).
        for i in (0..scene.len())
            .rev()
            .filter(|&i| parent_of(&scene[i]).is_none())
        {
            keep[i] = self.charge(&scene[i], &mut ledger);
        }
        let kept: Vec<SurfaceKey> = (0..scene.len())
            .filter(|&i| keep[i])
            .map(|i| scene[i].key)
            .collect();
        for i in (0..scene.len()).rev() {
            if let Some(parent) = parent_of(&scene[i]) {
                keep[i] = kept.contains(&parent) && self.charge(&scene[i], &mut ledger);
            }
        }
        let mut out = (Vec::with_capacity(scene.len()), Vec::new());
        for (p, k) in scene.into_iter().zip(keep) {
            if k {
                out.0.push(p);
            } else {
                out.1.push(p.key);
            }
        }
        out
    }

    /// Charges one surface; false if it is left out.
    fn charge(&self, p: &Placed, ledger: &mut Ledger) -> bool {
        let Some(c) = self.clients.get(&p.key.client) else {
            return true;
        };
        if is_system(c.rights) || c.rights.contains(Rights::SYSTEM_DIALOG) {
            return true;
        }
        let uid = c.conn.peer.uid;
        let focused = self.focus == Some(p.key);
        if p.window.is_some() {
            let n = ledger.windows.entry(uid).or_default();
            *n += 1;
            if *n > ledger.cap && !focused {
                return false;
            }
        }
        let cost = self.cost(p);
        let used = ledger.per_uid.entry(uid).or_default();
        if !focused && (*used + cost > ledger.budget || cost > ledger.remaining) {
            return false;
        }
        *used += cost;
        ledger.remaining = (ledger.remaining - cost).max(0);
        true
    }

    /// What a surface costs beyond opaque copies (logical points², visible
    /// part only).
    fn cost(&self, p: &Placed) -> i64 {
        let bounds = self.layout.bounds();
        let area = |r: crate::geom::Rect| r.intersect(&bounds).map_or(0, |r| r.area());
        let translucent = self.surface(p.key).is_some_and(|s| !s.opaque);
        let content = if translucent { 0 } else { area(p.content) };
        match p.window.and_then(|i| self.windows.get(i)) {
            Some(w) => (area(w.painted_area()) - content).max(0),
            None if translucent => area(p.content),
            None => 0,
        }
    }
}
