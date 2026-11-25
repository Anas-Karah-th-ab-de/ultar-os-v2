//! Parser for the SVG path subset the icon set uses: `M L H V C A Z` in absolute and relative
//! form, with implicit command repetition. Curves and arcs are flattened to polylines.

use crate::geom::Point;
use std::f32::consts::PI;
use std::fmt;

/// Most points one icon path may flatten to.
pub const MAX_POINTS: usize = 4096;
const CUBIC_STEPS: usize = 12;
/// Arc segments per full turn.
const ARC_STEPS_PER_TURN: f32 = 32.0;

/// One connected stretch of a path.
#[derive(Debug, Clone, PartialEq)]
pub struct Subpath {
    /// The corner points in path units.
    pub points: Vec<Point>,
    /// `true` when the last point connects back to the first.
    pub closed: bool,
}

/// Why a path string was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// A command letter outside the supported subset.
    UnsupportedCommand {
        /// The letter.
        command: char,
        /// Byte offset in the path string.
        at: usize,
    },
    /// A number was expected.
    ExpectedNumber {
        /// Byte offset in the path string.
        at: usize,
    },
    /// An arc flag must be `0` or `1`.
    InvalidFlag {
        /// Byte offset in the path string.
        at: usize,
    },
    /// Drawing started without a move command.
    MissingMove,
    /// The path flattens to more than [`MAX_POINTS`] points.
    TooLong,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::UnsupportedCommand { command, at } => {
                write!(f, "unsupported path command `{command}` at byte {at}")
            }
            PathError::ExpectedNumber { at } => write!(f, "number expected at byte {at}"),
            PathError::InvalidFlag { at } => write!(f, "arc flag must be 0 or 1 at byte {at}"),
            PathError::MissingMove => write!(f, "path does not start with a move command"),
            PathError::TooLong => write!(f, "path has more than {MAX_POINTS} points"),
        }
    }
}

impl std::error::Error for PathError {}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn skip_separators(&mut self) {
        while matches!(
            self.bytes.get(self.at),
            Some(b' ' | b',' | b'\t' | b'\n' | b'\r')
        ) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_separators();
        self.bytes.get(self.at).copied()
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Result<f32, PathError> {
        self.skip_separators();
        let start = self.at;
        if matches!(self.bytes.get(self.at), Some(b'-' | b'+')) {
            self.at += 1;
        }
        let mut count = self.digits();
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            count += self.digits();
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("");
        match text.parse::<f32>() {
            Ok(value) if count > 0 && value.is_finite() => Ok(value),
            _ => Err(PathError::ExpectedNumber { at: start }),
        }
    }

    fn flag(&mut self) -> Result<bool, PathError> {
        self.skip_separators();
        let at = self.at;
        self.at += 1;
        match self.bytes.get(at) {
            Some(b'0') => Ok(false),
            Some(b'1') => Ok(true),
            _ => Err(PathError::InvalidFlag { at }),
        }
    }

    fn starts_number(&mut self) -> bool {
        matches!(self.peek(), Some(b'0'..=b'9' | b'-' | b'+' | b'.'))
    }
}

#[derive(Default)]
struct Builder {
    done: Vec<Subpath>,
    current: Vec<Point>,
    pen: Point,
    total: usize,
    /// Second control point of the last cubic (for the smooth `S` command).
    last_control: Option<Point>,
}

impl Builder {
    fn push(&mut self, p: Point) -> Result<(), PathError> {
        if self.current.is_empty() {
            return Err(PathError::MissingMove);
        }
        self.total += 1;
        if self.total > MAX_POINTS {
            return Err(PathError::TooLong);
        }
        self.current.push(p);
        self.pen = p;
        Ok(())
    }

    fn finish(&mut self, closed: bool) {
        if !self.current.is_empty() {
            let points = std::mem::take(&mut self.current);
            self.pen = points[0];
            self.done.push(Subpath { points, closed });
        }
    }

    fn move_to(&mut self, p: Point) {
        self.finish(false);
        self.current.push(p);
        self.pen = p;
        self.total += 1;
    }

    fn cubic(&mut self, c1: Point, c2: Point, end: Point) -> Result<(), PathError> {
        let start = self.pen;
        for step in 1..=CUBIC_STEPS {
            let t = step as f32 / CUBIC_STEPS as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.push(Point::new(
                a * start.x + b * c1.x + c * c2.x + d * end.x,
                a * start.y + b * c1.y + c * c2.y + d * end.y,
            ))?;
        }
        Ok(())
    }

    /// Elliptical arc from the pen to `end` (SVG implementation notes, endpoint to centre form).
    fn arc(
        &mut self,
        radii: (f32, f32),
        rotation: f32,
        flags: (bool, bool),
        end: Point,
    ) -> Result<(), PathError> {
        let start = self.pen;
        let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
        if rx == 0.0 || ry == 0.0 || start == end {
            return self.push(end);
        }
        // libm, not the system maths library: the same pixels on every machine (snapshot tests).
        let (sin, cos) = libm::sincosf(rotation.to_radians());
        let (hx, hy) = ((start.x - end.x) / 2.0, (start.y - end.y) / 2.0);
        let (x1, y1) = (cos * hx + sin * hy, -sin * hx + cos * hy);
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        if lambda > 1.0 {
            rx *= lambda.sqrt();
            ry *= lambda.sqrt();
        }
        let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let numerator = (rx * rx * ry * ry - denominator).max(0.0);
        let sign = if flags.0 != flags.1 { 1.0 } else { -1.0 };
        let coef = sign * (numerator / denominator).sqrt();
        let (cx1, cy1) = (coef * rx * y1 / ry, -coef * ry * x1 / rx);
        let cx = cos * cx1 - sin * cy1 + (start.x + end.x) / 2.0;
        let cy = sin * cx1 + cos * cy1 + (start.y + end.y) / 2.0;
        let theta = libm::atan2f((y1 - cy1) / ry, (x1 - cx1) / rx);
        let mut delta = libm::atan2f((-y1 - cy1) / ry, (-x1 - cx1) / rx) - theta;
        if flags.1 && delta < 0.0 {
            delta += 2.0 * PI;
        } else if !flags.1 && delta > 0.0 {
            delta -= 2.0 * PI;
        }
        let steps = ((delta.abs() / (2.0 * PI)) * ARC_STEPS_PER_TURN)
            .ceil()
            .max(2.0) as usize;
        for step in 1..steps {
            let (s, c) = libm::sincosf(theta + delta * step as f32 / steps as f32);
            let (ex, ey) = (rx * c, ry * s);
            self.push(Point::new(
                cos * ex - sin * ey + cx,
                sin * ex + cos * ey + cy,
            ))?;
        }
        self.push(end)
    }
}

/// A cubic curve to `end` that remembers its second control point (for a following `S`).
fn curve_to(builder: &mut Builder, c1: Point, (c2, end): (Point, Point)) -> Result<(), PathError> {
    builder.cubic(c1, c2, end)?;
    builder.last_control = Some(c2);
    Ok(())
}

/// Reads one drawing command's arguments and applies it.
fn apply(
    reader: &mut Reader<'_>,
    builder: &mut Builder,
    command: u8,
    at: usize,
) -> Result<(), PathError> {
    let origin = [Point::default(), builder.pen][usize::from(command.is_ascii_lowercase())];
    let point = |reader: &mut Reader<'_>| -> Result<Point, PathError> {
        Ok(Point::new(
            origin.x + reader.number()?,
            origin.y + reader.number()?,
        ))
    };
    let upper = command.to_ascii_uppercase();
    if !matches!(upper, b'C' | b'S') {
        builder.last_control = None;
    }
    match upper {
        b'M' => point(reader).map(|p| builder.move_to(p)),
        b'L' => point(reader).and_then(|p| builder.push(p)),
        b'H' => {
            let x = origin.x + reader.number()?;
            builder.push(Point::new(x, builder.pen.y))
        }
        b'V' => {
            let y = origin.y + reader.number()?;
            builder.push(Point::new(builder.pen.x, y))
        }
        b'C' => {
            let c1 = point(reader)?;
            curve_to(builder, c1, (point(reader)?, point(reader)?))
        }
        b'S' => {
            // The first control point mirrors the last one of the previous curve.
            let pen = builder.pen;
            let c1 = builder
                .last_control
                .map_or(pen, |c| Point::new(2.0 * pen.x - c.x, 2.0 * pen.y - c.y));
            curve_to(builder, c1, (point(reader)?, point(reader)?))
        }
        b'A' => {
            let radii = (reader.number()?, reader.number()?);
            let rotation = reader.number()?;
            let flags = (reader.flag()?, reader.flag()?);
            let end = point(reader)?;
            builder.arc(radii, rotation, flags, end)
        }
        _ => Err(PathError::UnsupportedCommand {
            command: command as char,
            at,
        }),
    }
}

/// Parses a path string into flattened subpaths.
pub fn parse(d: &str) -> Result<Vec<Subpath>, PathError> {
    let mut reader = Reader {
        bytes: d.as_bytes(),
        at: 0,
    };
    let mut builder = Builder::default();
    while let Some(command) = reader.peek() {
        let at = reader.at;
        reader.at += 1;
        if matches!(command, b'Z' | b'z') {
            builder.finish(true);
            // A path may continue after closing without a new move command.
            builder.current.push(builder.pen);
            continue;
        }
        if !command.is_ascii_alphabetic() {
            return Err(PathError::ExpectedNumber { at });
        }
        apply(&mut reader, &mut builder, command, at)?;
        // Further coordinate groups repeat the command; after a move they are line-tos.
        let repeat = match command {
            b'M' => b'L',
            b'm' => b'l',
            other => other,
        };
        while reader.starts_number() {
            let at = reader.at;
            apply(&mut reader, &mut builder, repeat, at)?;
        }
    }
    builder.finish(false);
    builder.done.retain(|s| s.points.len() > 1);
    Ok(builder.done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, x: f32, y: f32) -> bool {
        (a.x - x).abs() < 0.01 && (a.y - y).abs() < 0.01
    }

    #[test]
    fn lines_relative_and_implicit_repeats() {
        let paths = parse("M6 6l12 12M18 6L6 18").unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(
            paths[0].points,
            vec![Point::new(6.0, 6.0), Point::new(18.0, 18.0)]
        );
        assert!(!paths[0].closed);
        let chevron = parse("M9 6l6 6-6 6").unwrap();
        assert_eq!(chevron[0].points.len(), 3);
        assert!(close(chevron[0].points[2], 9.0, 18.0));
        let compact = parse("M5 5h.01M3 8h15v8H3z").unwrap();
        assert!(close(compact[0].points[1], 5.01, 5.0));
        assert!(compact[1].closed);
        assert!(close(compact[1].points[3], 3.0, 16.0));
        let implicit = parse("M1 1 2 2 3 3").unwrap();
        assert_eq!(implicit[0].points.len(), 3);
    }

    #[test]
    fn arcs_end_where_they_should_and_stay_on_the_circle() {
        // A full circle of radius 7 around (11, 11) drawn as two half arcs.
        let circle = parse("M11 4a7 7 0 1 0 0 14a7 7 0 0 0 0-14z").unwrap();
        assert!(circle[0].closed);
        for p in &circle[0].points {
            let r = ((p.x - 11.0).powi(2) + (p.y - 11.0).powi(2)).sqrt();
            assert!((r - 7.0).abs() < 0.01, "{p:?} r={r}");
        }
        assert!(circle[0].points.iter().any(|p| close(*p, 11.0, 18.0)));
        assert!(circle[0].points.iter().any(|p| p.x < 4.1));
        // Compact flags without separators, and a too-small radius that is scaled up.
        let flags = parse("M0 0a5 5 0 0110 0").unwrap();
        assert!(close(*flags[0].points.last().unwrap(), 10.0, 0.0));
        assert!(
            flags[0].points.iter().all(|p| p.y <= 0.01),
            "sweep flag 1 goes over the top"
        );
        let scaled = parse("M0 0A1 1 0 0 0 10 0").unwrap();
        assert!(scaled[0].points.iter().any(|p| (p.y - 5.0).abs() < 0.1));
        let degenerate = parse("M0 0A0 5 0 0 0 4 4").unwrap();
        assert_eq!(degenerate[0].points.len(), 2);
    }

    #[test]
    fn cubic_reaches_its_end_point() {
        let curve = parse("M12 3c3 3 3 15 0 18").unwrap();
        assert_eq!(curve[0].points.len(), 1 + CUBIC_STEPS);
        assert!(close(*curve[0].points.last().unwrap(), 12.0, 21.0));
        assert!(curve[0].points.iter().any(|p| p.x > 13.5));
        let absolute = parse("M0 0C0 10 10 10 10 0").unwrap();
        assert!(close(absolute[0].points[CUBIC_STEPS / 2], 5.0, 7.5));
    }

    #[test]
    fn errors_are_typed() {
        assert_eq!(parse("L1 1"), Err(PathError::MissingMove));
        assert_eq!(
            parse("M1 1Q2 2 3 3"),
            Err(PathError::UnsupportedCommand {
                command: 'Q',
                at: 4
            })
        );
        assert_eq!(parse("M1"), Err(PathError::ExpectedNumber { at: 2 }));
        assert_eq!(parse("M1 x"), Err(PathError::ExpectedNumber { at: 3 }));
        assert_eq!(parse("5 5"), Err(PathError::ExpectedNumber { at: 0 }));
        assert_eq!(
            parse("M0 0a5 5 0 2 0 1 1"),
            Err(PathError::InvalidFlag { at: 11 })
        );
        let long = format!("M0 0{}", "l1 1".repeat(MAX_POINTS));
        assert_eq!(parse(&long), Err(PathError::TooLong));
        assert_eq!(parse(""), Ok(vec![]));
        assert!(PathError::TooLong.to_string().contains("4096"));
        assert!(PathError::MissingMove.to_string().contains("move"));
    }
}
