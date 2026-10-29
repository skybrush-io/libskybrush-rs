//! A single segment of a trajectory.

use core::cell::OnceCell;

use crate::basic_types::Vector3WithYaw;
use crate::parsing::{read_i16, read_u16};
use crate::poly::{MAX_POLY_COEFFS, Poly, Poly4D};

/// The number of control points of an axis, including the start point, from
/// its two bits in the header of the segment: constant (1), linear (2), cubic
/// Bézier (4) or a Bézier curve of degree 7 (8).
pub(crate) fn num_coords(bits: u8) -> usize {
    1 << (bits & 0x03)
}

/// The number of bytes of the coordinates of a segment with the given header,
/// including the legacy yaw axis.
pub(crate) fn coords_length(header: u8) -> usize {
    [0, 2, 4, 6]
        .into_iter()
        .map(|shift| 2 * (num_coords(header >> shift) - 1))
        .sum()
}

/// A segment of a trajectory, with its polynomials calculated lazily.
#[derive(Clone, Debug)]
pub struct TrajectorySegment {
    start_time_msec: u32,
    end_time_msec: u32,
    duration_msec: u32,
    start_time_sec: f32,
    end_time_sec: f32,
    duration_sec: f32,
    start: Vector3WithYaw,
    end: Vector3WithYaw,
    /// The control points along the X, Y and Z axes, including the start
    /// point, and their numbers.
    points: [[f32; MAX_POLY_COEFFS]; 3],
    num_points: [usize; 3],
    poly: OnceCell<Poly4D>,
    dpoly: OnceCell<Poly4D>,
    ddpoly: OnceCell<Poly4D>,
}

/// Segments are equal if they have the same times and control points; the
/// polynomials that were calculated so far do not matter.
impl PartialEq for TrajectorySegment {
    fn eq(&self, other: &Self) -> bool {
        self.start_time_msec == other.start_time_msec
            && self.end_time_msec == other.end_time_msec
            && self.duration_msec == other.duration_msec
            && self.start_time_sec == other.start_time_sec
            && self.end_time_sec == other.end_time_sec
            && self.duration_sec == other.duration_sec
            && self.start == other.start
            && self.end == other.end
            && self.points == other.points
            && self.num_points == other.num_points
    }
}

impl TrajectorySegment {
    /// The segment after the end of a trajectory (or of an empty one), which
    /// holds the given position forever.
    pub(crate) fn hold(start_time_msec: u32, position: Vector3WithYaw) -> Self {
        let mut points = [[0.0; MAX_POLY_COEFFS]; 3];
        points[0][0] = position.x;
        points[1][0] = position.y;
        points[2][0] = position.z;
        Self {
            start_time_msec,
            end_time_msec: u32::MAX,
            duration_msec: u32::MAX - start_time_msec,
            start_time_sec: msec_to_sec(start_time_msec),
            end_time_sec: f32::INFINITY,
            duration_sec: f32::INFINITY,
            start: position,
            end: position,
            points,
            num_points: [1; 3],
            poly: OnceCell::from(Poly4D::constant(position)),
            dpoly: OnceCell::new(),
            ddpoly: OnceCell::new(),
        }
    }

    /// Decodes the segment at the start of `bytes`, starting at the given time
    /// and position. Returns the segment and the number of bytes it occupies,
    /// or `None` if the bytes end before the end of the segment.
    pub(crate) fn decode(
        bytes: &[u8],
        start_time_msec: u32,
        start: Vector3WithYaw,
        scale: f32,
    ) -> Option<(Self, usize)> {
        let (&header, _) = bytes.split_first()?;
        let duration_msec = u32::from(read_u16(bytes, 1)?);
        let end_time_msec = start_time_msec.saturating_add(duration_msec);

        let mut offset = 3;
        let mut points = [[0.0; MAX_POLY_COEFFS]; 3];
        let mut num_points = [1; 3];
        let mut end = start;
        let starts = [start.x, start.y, start.z];
        let ends = [&mut end.x, &mut end.y, &mut end.z];
        for (axis, ((points, count), end)) in
            points.iter_mut().zip(&mut num_points).zip(ends).enumerate()
        {
            let n = num_coords(header >> (2 * axis));
            *count = n;
            let (first, rest) = points.split_first_mut()?;
            *first = starts.get(axis).copied().unwrap_or_default();
            for point in rest.iter_mut().take(n - 1) {
                *point = f32::from(read_i16(bytes, offset)?) * scale;
                offset += 2;
            }
            *end = points.get(n - 1).copied().unwrap_or_default();
        }
        // The legacy yaw coordinates are ignored, but they occupy bytes
        offset += 2 * (num_coords(header >> 6) - 1);
        if offset > bytes.len() {
            return None;
        }

        let segment = Self {
            start_time_msec,
            end_time_msec,
            duration_msec,
            start_time_sec: msec_to_sec(start_time_msec),
            end_time_sec: msec_to_sec(end_time_msec),
            duration_sec: msec_to_sec(duration_msec),
            start,
            end,
            points,
            num_points,
            poly: OnceCell::new(),
            dpoly: OnceCell::new(),
            ddpoly: OnceCell::new(),
        };
        Some((segment, offset))
    }

    /// The start time of the segment since the start of the trajectory, in
    /// milliseconds.
    #[must_use]
    pub const fn start_time_msec(&self) -> u32 {
        self.start_time_msec
    }

    /// The end time of the segment since the start of the trajectory, in
    /// milliseconds; `u32::MAX` for the segment after the end of the
    /// trajectory.
    #[must_use]
    pub const fn end_time_msec(&self) -> u32 {
        self.end_time_msec
    }

    /// The duration of the segment, in milliseconds.
    #[must_use]
    pub const fn duration_msec(&self) -> u32 {
        self.duration_msec
    }

    /// The start time of the segment since the start of the trajectory, in
    /// seconds.
    #[must_use]
    pub const fn start_time_sec(&self) -> f32 {
        self.start_time_sec
    }

    /// The end time of the segment since the start of the trajectory, in
    /// seconds; infinite for the segment after the end of the trajectory.
    #[must_use]
    pub const fn end_time_sec(&self) -> f32 {
        self.end_time_sec
    }

    /// The duration of the segment, in seconds.
    #[must_use]
    pub const fn duration_sec(&self) -> f32 {
        self.duration_sec
    }

    /// The first point of the segment.
    #[must_use]
    pub const fn start(&self) -> Vector3WithYaw {
        self.start
    }

    /// The last point of the segment.
    #[must_use]
    pub const fn end(&self) -> Vector3WithYaw {
        self.end
    }

    /// The control points of the segment along an axis (0, 1 or 2 for X, Y
    /// or Z), including the start point.
    pub(crate) fn control_points(&self, axis: usize) -> &[f32] {
        let count = self.num_points.get(axis).copied().unwrap_or(0);
        self.points
            .get(axis)
            .and_then(|points| points.get(..count))
            .unwrap_or_default()
    }

    /// The polynomial of the segment. Its `[0, 1]` interval is mapped to the
    /// time interval of the segment.
    pub fn poly(&self) -> &Poly4D {
        let (points, num_points, yaw) = (&self.points, &self.num_points, self.start.yaw);
        self.poly.get_or_init(|| {
            let axis = |index: usize| {
                let points = points.get(index).map_or(&[][..], |points| {
                    let count = num_points.get(index).copied().unwrap_or(1);
                    points.get(..count).unwrap_or(points)
                });
                Poly::bezier(1.0, points)
            };
            Poly4D {
                x: axis(0),
                y: axis(1),
                z: axis(2),
                yaw: Poly::constant(yaw),
            }
        })
    }

    /// The first derivative of the polynomial of the segment, with respect to
    /// time in seconds.
    pub fn dpoly(&self) -> &Poly4D {
        self.dpoly.get_or_init(|| self.derivative_of(*self.poly()))
    }

    /// The second derivative of the polynomial of the segment, with respect to
    /// time in seconds.
    pub fn ddpoly(&self) -> &Poly4D {
        self.ddpoly
            .get_or_init(|| self.derivative_of(*self.dpoly()))
    }

    /// The derivative of a polynomial of the segment with respect to time in
    /// seconds. For segments that are too short, it is left with respect to
    /// the relative time, like in the C library.
    fn derivative_of(&self, poly: Poly4D) -> Poly4D {
        let mut result = poly;
        result.deriv();
        if self.duration_sec.abs() > 1.0e-6 {
            result.scale(1.0 / self.duration_sec);
        }
        result
    }
}

/// Converts milliseconds to seconds, like the C library: the milliseconds are
/// rounded to `f32` first.
#[expect(
    clippy::cast_precision_loss,
    reason = "the conversion of the C library"
)]
pub(crate) fn msec_to_sec(msec: u32) -> f32 {
    msec as f32 / 1000.0
}

#[cfg(test)]
mod tests {
    use crate::basic_types::Vector3WithYaw;
    use crate::trajectory::{TrajectoryBuilder, TrajectoryPlayer};

    #[test]
    fn equality_ignores_the_calculated_polynomials() {
        let mut builder = TrajectoryBuilder::new(1).unwrap();
        builder
            .set_start_position(Vector3WithYaw::default())
            .unwrap();
        let target = Vector3WithYaw {
            x: 1000.0,
            ..Vector3WithYaw::default()
        };
        builder.append_line(target, 1000).unwrap();
        let trajectory = builder.build().unwrap();

        let player = TrajectoryPlayer::new(&trajectory);
        let fresh = player.current_segment().clone();
        let used = player.current_segment();
        used.ddpoly();
        assert_eq!(*used, fresh);
    }
}
