//! One remote-view session of the source: look at the screen, send what changed, inject
//! what the viewer did. Pure logic over three small traits ([`Screen`], [`Inject`],
//! [`Sink`]), so the whole behaviour is tested without a compositor, a kernel or a bus.

use crate::diff::{Differ, Rect};
use crate::input::Inject;
use crate::pace::Pacer;
use crate::screen::{Frame, Screen, ScreenError};
use std::time::Duration;
use ultar_bus::messages::mesh_view::RemoteInput;

/// Most rectangles tried in one look.
pub const RECTS_PER_LOOK: usize = 64;

/// Where the rectangles go (the mesh).
pub trait Sink {
    /// Offers a rectangle; `true` when the mesh took it (`false`: slow link, try again).
    fn picture(&mut self, rect: Rect, data: Vec<u8>) -> bool;
}

/// What the loop does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Wait this long, then look again.
    Wait(Duration),
    /// The screen locked: the session ends now.
    Locked,
    /// The screen is gone (compositor away, resized): the session ends now.
    Gone,
}

/// A session.
pub struct Engine<S: Screen, I: Inject> {
    screen: S,
    inject: I,
    differ: Differ,
    pacer: Pacer,
}

impl<S: Screen, I: Inject> Engine<S, I> {
    /// Looks at the screen once to learn its size; the picture is announced to the viewer
    /// by the caller (`mesh.view_ready`) and streamed by [`Engine::step`].
    pub fn begin(
        mut screen: S,
        injector: impl FnOnce((u32, u32)) -> I,
    ) -> Result<(Self, (u32, u32)), ScreenError> {
        let first = screen.grab(&mut || {})?;
        let size = (first.width, first.height);
        let engine = Self {
            screen,
            inject: injector(size),
            differ: Differ::new(size.0, size.1),
            pacer: Pacer::default(),
        };
        Ok((engine, size))
    }

    /// One look: the viewer's `inputs` are injected as soon as the screen proved to be
    /// unlocked, changed rectangles go to `sink`.
    pub fn step(
        &mut self,
        inputs: &mut dyn FnMut() -> Option<RemoteInput>,
        sink: &mut dyn Sink,
    ) -> Step {
        let grabbed = {
            let Self { screen, inject, .. } = self;
            screen.grab(&mut || {
                while let Some(input) = inputs() {
                    inject.apply(input);
                }
            })
        };
        let frame: Frame = match grabbed {
            Ok(f) if (f.width, f.height) == self.differ.size() => f,
            Ok(_) => return self.finish(Step::Gone),
            Err(ScreenError::Locked) => return self.finish(Step::Locked),
            Err(ScreenError::Gone) => return self.finish(Step::Gone),
        };
        let rects = self.differ.changed(&frame.data, RECTS_PER_LOOK);
        let mut dropped = false;
        for rect in &rects {
            let data = self.differ.extract(&frame.data, *rect);
            if sink.picture(*rect, data) {
                self.differ.commit(&frame.data, *rect);
            } else {
                dropped = true;
                break;
            }
        }
        match (rects.is_empty(), dropped) {
            (_, true) => self.pacer.slow(),
            (true, false) => self.pacer.idle(),
            (false, false) => self.pacer.busy(),
        }
        Step::Wait(self.pacer.interval())
    }

    /// Ends the session: nothing the viewer pressed stays pressed.
    pub fn finish(&mut self, step: Step) -> Step {
        self.inject.release_all();
        step
    }
}
