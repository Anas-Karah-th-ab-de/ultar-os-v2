//! Touch gestures on the pages: one finger pans, two fingers pinch (zoom).

use ultar_ui::event::Event;
use ultar_ui::geom::Point;

/// What the fingers do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// One finger moved by this much (logical points).
    Pan { dx: f32, dy: f32 },
    /// A second finger came down: the pinch starts.
    PinchStart,
    /// Two fingers: their distance at the start and now.
    PinchMove { start: f32, dist: f32 },
    /// One of the two fingers left.
    PinchEnd,
}

/// The fingers on the page area.
#[derive(Debug, Clone, Default)]
pub struct Touches {
    fingers: Vec<(u64, Point)>,
    start_dist: f32,
}

fn dist(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

impl Touches {
    /// Takes one touch event; `None` for events that are no gesture.
    pub fn handle(&mut self, event: &Event) -> Option<Gesture> {
        match *event {
            Event::TouchDown { id, pos } => {
                self.fingers.retain(|f| f.0 != id);
                self.fingers.push((id, pos));
                self.fingers.truncate(2);
                if self.fingers.len() == 2 {
                    self.start_dist = dist(self.fingers[0].1, self.fingers[1].1);
                    return Some(Gesture::PinchStart);
                }
                None
            }
            Event::TouchMove { id, pos } => {
                let at = self.fingers.iter().position(|f| f.0 == id)?;
                let before = self.fingers[at].1;
                self.fingers[at].1 = pos;
                if self.fingers.len() == 2 {
                    Some(Gesture::PinchMove {
                        start: self.start_dist,
                        dist: dist(self.fingers[0].1, self.fingers[1].1),
                    })
                } else {
                    Some(Gesture::Pan {
                        dx: pos.x - before.x,
                        dy: pos.y - before.y,
                    })
                }
            }
            Event::TouchUp { id, .. } | Event::TouchCancel { id } => {
                let was_pinch = self.fingers.len() == 2;
                self.fingers.retain(|f| f.0 != id);
                if was_pinch && self.fingers.len() == 1 {
                    return Some(Gesture::PinchEnd);
                }
                if id == 0 {
                    // a cancel that names no finger ends everything
                    self.fingers.clear();
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn one_finger_pans() {
        let mut t = Touches::default();
        assert_eq!(t.handle(&Event::TouchDown { id: 1, pos: p(10.0, 10.0) }), None);
        assert_eq!(
            t.handle(&Event::TouchMove { id: 1, pos: p(12.0, 30.0) }),
            Some(Gesture::Pan { dx: 2.0, dy: 20.0 })
        );
        assert_eq!(t.handle(&Event::TouchUp { id: 1, pos: p(12.0, 30.0) }), None);
        assert_eq!(t.handle(&Event::TouchMove { id: 1, pos: p(0.0, 0.0) }), None);
    }

    #[test]
    fn two_fingers_pinch_and_the_rest_pans_again() {
        let mut t = Touches::default();
        t.handle(&Event::TouchDown { id: 1, pos: p(0.0, 0.0) });
        assert_eq!(
            t.handle(&Event::TouchDown { id: 2, pos: p(100.0, 0.0) }),
            Some(Gesture::PinchStart)
        );
        assert_eq!(
            t.handle(&Event::TouchMove { id: 2, pos: p(200.0, 0.0) }),
            Some(Gesture::PinchMove { start: 100.0, dist: 200.0 })
        );
        assert_eq!(
            t.handle(&Event::TouchUp { id: 2, pos: p(200.0, 0.0) }),
            Some(Gesture::PinchEnd)
        );
        assert_eq!(
            t.handle(&Event::TouchMove { id: 1, pos: p(0.0, 5.0) }),
            Some(Gesture::Pan { dx: 0.0, dy: 5.0 })
        );
    }

    #[test]
    fn a_third_finger_is_ignored() {
        let mut t = Touches::default();
        t.handle(&Event::TouchDown { id: 1, pos: p(0.0, 0.0) });
        t.handle(&Event::TouchDown { id: 2, pos: p(10.0, 0.0) });
        t.handle(&Event::TouchDown { id: 3, pos: p(20.0, 0.0) });
        assert_eq!(t.handle(&Event::TouchMove { id: 3, pos: p(50.0, 0.0) }), None);
    }
}
