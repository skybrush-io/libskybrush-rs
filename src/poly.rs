//! Polynomials of degree at most 7, and 4D polynomials along the X, Y, Z and
//! yaw coordinates.
#![allow(
    clippy::float_cmp,
    reason = "exact comparisons of floats, as in the C library"
)]
#![allow(
    clippy::many_single_char_names,
    reason = "the coefficients have their usual names in the formulas"
)]

use core::f32::consts::PI;
use core::ops::Deref;
use core::{error, fmt};

use libm::{atan2f, cbrtf, cosf, sqrtf};

use crate::basic_types::{Interval, Vector3WithYaw};

/// The maximum degree of a polynomial.
pub const MAX_POLY_DEGREE: usize = 7;

/// The maximum number of coefficients of a polynomial.
pub const MAX_POLY_COEFFS: usize = MAX_POLY_DEGREE + 1;

/// Factorials of the numbers up to the maximum degree.
const FACTORIALS: [f32; MAX_POLY_COEFFS] = [1.0, 1.0, 2.0, 6.0, 24.0, 120.0, 720.0, 5040.0];

/// Whether a value is zero for the purposes of the polynomial operations,
/// i.e. zero or subnormal.
fn is_zero(value: f32) -> bool {
    value.abs() < f32::MIN_POSITIVE
}

/// Why an operation on a polynomial failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolyError {
    /// The operation is not implemented for polynomials of this degree.
    UnsupportedDegree(u8),
}

impl fmt::Display for PolyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedDegree(degree) => {
                write!(f, "not implemented for polynomials of degree {degree}")
            }
        }
    }
}

impl error::Error for PolyError {}

/// The real roots of a polynomial of degree at most 3.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Roots {
    values: [f32; 3],
    len: usize,
}

impl Roots {
    fn new(roots: &[f32]) -> Self {
        let mut result = Self::default();
        for (slot, &root) in result.values.iter_mut().zip(roots) {
            *slot = root;
            result.len += 1;
        }
        result
    }
}

impl Deref for Roots {
    type Target = [f32];

    fn deref(&self) -> &[f32] {
        self.values.get(..self.len).unwrap_or_default()
    }
}

/// A one-dimensional polynomial of degree at most 7.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Poly {
    /// The coefficients; the ones from `num_coeffs` on are zero.
    coeffs: [f32; MAX_POLY_COEFFS],
    /// The number of coefficients. Zero means the all-zero polynomial, 1 a
    /// constant, 2 a linear polynomial and so on.
    num_coeffs: usize,
}

impl Poly {
    /// A polynomial with the given coefficients, constant term first. Only
    /// the first [`MAX_POLY_COEFFS`] coefficients are used.
    #[must_use]
    pub fn new(coeffs: &[f32]) -> Self {
        let mut result = Self::default();
        for (slot, &coeff) in result.coeffs.iter_mut().zip(coeffs) {
            *slot = coeff;
            result.num_coeffs += 1;
        }
        result
    }

    /// The constant zero polynomial.
    #[must_use]
    pub fn zero() -> Self {
        Self::constant(0.0)
    }

    /// A constant polynomial.
    #[must_use]
    pub fn constant(value: f32) -> Self {
        Self::new(&[value])
    }

    /// A linear polynomial with `p(0) = x0` and `p(duration) = x1`. If the
    /// duration is too short, the polynomial is the constant average of the
    /// two values.
    #[must_use]
    pub fn linear(duration: f32, x0: f32, x1: f32) -> Self {
        if duration.abs() >= f32::EPSILON {
            Self::new(&[x0, (x1 - x0) / duration])
        } else {
            #[expect(
                clippy::manual_midpoint,
                reason = "as in C, which overflows where f32::midpoint would not"
            )]
            let average = (x0 + x1) / 2.0;
            Self::new(&[average, 0.0])
        }
    }

    /// A polynomial from Bézier control points, going from the first control
    /// point at 0 to the last one at `duration`. Only the first
    /// [`MAX_POLY_COEFFS`] control points are used.
    #[must_use]
    pub fn bezier(duration: f32, points: &[f32]) -> Self {
        match points {
            [] => Self::zero(),
            [x] => Self::constant(*x),
            [x0, x1] => Self::linear(duration, *x0, *x1),
            _ => {
                let points = points.get(..MAX_POLY_COEFFS).unwrap_or(points);
                let n = points.len() - 1;
                let mut result = Self::default();
                for (j, slot) in result.coeffs.iter_mut().enumerate().take(n + 1) {
                    let mut coeff = 0.0;
                    let mut sign = if j % 2 == 0 { 1.0 } else { -1.0 };
                    for (i, &point) in points.iter().enumerate().take(j + 1) {
                        coeff += sign * point / factorial(i) / factorial(j - i);
                        sign = -sign;
                    }
                    *slot = coeff * factorial(n) / factorial(n - j);
                }
                result.num_coeffs = n + 1;
                result.stretch(duration);
                result
            }
        }
    }

    /// A quadratic polynomial from Bézier control points.
    #[must_use]
    pub fn quadratic_bezier(duration: f32, u: f32, v: f32, w: f32) -> Self {
        Self::bezier(duration, &[u, v, w])
    }

    /// A cubic polynomial from Bézier control points.
    #[must_use]
    pub fn cubic_bezier(duration: f32, u: f32, v: f32, w: f32, x: f32) -> Self {
        Self::bezier(duration, &[u, v, w, x])
    }

    /// The coefficients of the polynomial, constant term first.
    #[must_use]
    pub fn coeffs(&self) -> &[f32] {
        self.coeffs.get(..self.num_coeffs).unwrap_or(&self.coeffs)
    }

    /// The degree of the polynomial; zero for constants and the all-zero
    /// polynomial.
    #[must_use]
    pub fn degree(&self) -> usize {
        self.num_coeffs.saturating_sub(1)
    }

    /// Evaluates the polynomial with Horner's rule.
    #[must_use]
    pub fn eval(&self, t: f32) -> f32 {
        self.coeffs()
            .iter()
            .rev()
            .fold(0.0, |result, &coeff| result * t + coeff)
    }

    /// Evaluates the polynomial with Horner's rule in double precision.
    #[must_use]
    pub fn eval_double(&self, t: f64) -> f64 {
        self.coeffs()
            .iter()
            .rev()
            .fold(0.0, |result, &coeff| result * t + f64::from(coeff))
    }

    /// The minimum and maximum of the polynomial on the `[0, 1]` interval.
    ///
    /// # Errors
    ///
    /// Returns an error if the degree of the polynomial is above 3.
    pub fn extrema(&self) -> Result<Interval, PolyError> {
        // Leading zeros are ignored, but the polynomial is treated as linear
        // at least
        let mut coeffs = self.coeffs();
        while let [rest @ .., _, _, last] = coeffs
            && is_zero(*last)
        {
            coeffs = coeffs.get(..rest.len() + 2).unwrap_or(coeffs);
        }

        match *coeffs {
            [] => Ok(Interval { min: 0.0, max: 0.0 }),
            [c0] => Ok(Interval { min: c0, max: c0 }),
            [c0, c1] => Ok(if c1 > 0.0 {
                Interval {
                    min: c0,
                    max: c0 + c1,
                }
            } else {
                Interval {
                    min: c0 + c1,
                    max: c0,
                }
            }),
            [_, _, _] | [_, _, _, _] => {
                let mut deriv = *self;
                deriv.deriv();
                let roots = deriv.solve(0.0)?;

                let (at_zero, at_one) = (self.eval(0.0), self.eval(1.0));
                let mut result = if at_zero > at_one {
                    Interval {
                        min: at_one,
                        max: at_zero,
                    }
                } else {
                    Interval {
                        min: at_zero,
                        max: at_one,
                    }
                };
                for &root in roots.iter().rev() {
                    if (0.0..=1.0).contains(&root) {
                        let value = self.eval(root);
                        if value < result.min {
                            result.min = value;
                        }
                        if value > result.max {
                            result.max = value;
                        }
                    }
                }
                Ok(result)
            }
            _ => Err(PolyError::UnsupportedDegree(degree_of(coeffs))),
        }
    }

    /// Adds a constant to the polynomial.
    pub fn add_constant(&mut self, constant: f32) {
        if self.num_coeffs == 0 {
            *self = Self::constant(constant);
        } else if let Some(c0) = self.coeffs.first_mut() {
            *c0 += constant;
        }
    }

    /// Multiplies the polynomial by a factor.
    pub fn scale(&mut self, factor: f32) {
        for coeff in self.coeffs.iter_mut().take(self.num_coeffs) {
            *coeff *= factor;
        }
    }

    /// Replaces the polynomial with its derivative.
    pub fn deriv(&mut self) {
        if self.num_coeffs > 1 {
            for i in 1..self.num_coeffs {
                let next = self.coeffs.get(i).copied().unwrap_or_default();
                if let Some(coeff) = self.coeffs.get_mut(i - 1) {
                    *coeff = index_as_f32(i) * next;
                }
            }
            self.num_coeffs -= 1;
            if let Some(coeff) = self.coeffs.get_mut(self.num_coeffs) {
                *coeff = 0.0;
            }
        } else {
            *self = Self::zero();
        }
    }

    /// Stretches the time axis of the polynomial by a factor, so that the new
    /// polynomial at `factor * t` equals the old one at `t`.
    pub fn stretch(&mut self, factor: f32) {
        let factor = 1.0 / factor;
        let mut scale = factor;
        for coeff in self.coeffs.iter_mut().take(self.num_coeffs).skip(1) {
            *coeff *= scale;
            scale *= factor;
        }
    }

    /// The real roots of `p(t) = rhs`.
    ///
    /// Leading coefficients that are zero are ignored. A constant polynomial
    /// that equals `rhs` has a single root at zero.
    ///
    /// # Errors
    ///
    /// Returns an error if the degree of the polynomial is above 3.
    pub fn solve(&self, rhs: f32) -> Result<Roots, PolyError> {
        match *self.significant_coeffs() {
            [] => Ok(Roots::default()),
            [c0] => Ok(if is_zero(c0 - rhs) {
                Roots::new(&[0.0])
            } else {
                Roots::default()
            }),
            [c0, c1] => Ok(solve_linear(c1, c0 - rhs)),
            [c0, c1, c2] => Ok(solve_quadratic(c2, c1, c0 - rhs)),
            [c0, c1, c2, c3] => Ok(solve_cubic(c3, c2, c1, c0 - rhs)),
            ref coeffs => Err(PolyError::UnsupportedDegree(degree_of(coeffs))),
        }
    }

    /// A point in the `[0, 1]` interval where the polynomial equals the given
    /// value, or `None` if there is no such point. If there are several such
    /// points, the leftmost one is returned for polynomials of degree 2 and 3.
    ///
    /// Always `None` for polynomials of degree above 3.
    #[must_use]
    pub fn touches(&self, value: f32) -> Option<f32> {
        match *self.significant_coeffs() {
            [] | [_] => touches_constant(self.coeffs.first().copied().unwrap_or_default(), value),
            [b, a] => touches_linear(a, b, value),
            [c, b, a] => self.touches_quadratic(a, b, c, value),
            [d, c, b, a] => self.touches_cubic(a, b, c, d, value),
            _ => None,
        }
    }

    /// The coefficients without the leading zeros, keeping at least one.
    fn significant_coeffs(&self) -> &[f32] {
        let mut coeffs = self.coeffs();
        while let [rest @ .., _, last] = coeffs
            && is_zero(*last)
        {
            coeffs = coeffs.get(..=rest.len()).unwrap_or(coeffs);
        }
        coeffs
    }

    fn touches_quadratic(&self, a: f32, b: f32, c: f32, value: f32) -> Option<f32> {
        let value_at_zero = c;
        let value_at_one = a + b + c;

        if value_at_zero == value {
            return Some(0.0);
        }
        if value_at_one == value {
            return Some(1.0);
        }

        // Above the value at zero and non-decreasing in [0, 1]
        if value_at_zero > value && b >= 0.0 && a >= -b / 2.0 {
            return None;
        }
        // Below the value at zero and non-increasing in [0, 1]
        if value_at_zero < value && b <= 0.0 && a <= -b / 2.0 {
            return None;
        }

        self.leftmost_root_in_unit_interval(value)
    }

    fn touches_cubic(&self, a: f32, b: f32, c: f32, d: f32, value: f32) -> Option<f32> {
        let value_at_zero = d;
        let value_at_one = a + b + c + d;

        if value_at_zero == value {
            return Some(0.0);
        }
        if value_at_one == value {
            return Some(1.0);
        }

        // The derivative is 3ax^2 + 2bx + c; its extremum is at -b / 3a, with
        // a value of c - b^2 / 3a.
        let deriv_at_one = 3.0 * a + 2.0 * b + c;

        // Above the value at zero and non-decreasing in [0, 1]
        if value_at_zero > value && c >= 0.0 && deriv_at_one >= 0.0 {
            if a <= 0.0 || b > 0.0 || b < -3.0 * a {
                // Not convex, or the minimum of the derivative is not in (0, 1)
                return None;
            }
            if c - (b * b) / (3.0 * a) >= 0.0 {
                return None;
            }
        }

        // Below the value at zero and non-increasing in [0, 1]
        if value_at_zero < value && c <= 0.0 && deriv_at_one <= 0.0 {
            if a >= 0.0 || b < 0.0 || b > -3.0 * a {
                // Not concave, or the maximum of the derivative is not in (0, 1)
                return None;
            }
            if c - (b * b) / (3.0 * a) <= 0.0 {
                return None;
            }
        }

        self.leftmost_root_in_unit_interval(value)
    }

    fn leftmost_root_in_unit_interval(&self, value: f32) -> Option<f32> {
        let roots = self.solve(value).ok()?;
        let mut leftmost: Option<f32> = None;
        for &root in roots.iter() {
            if (0.0..=1.0).contains(&root) && leftmost.is_none_or(|leftmost| root < leftmost) {
                leftmost = Some(root);
            }
        }
        leftmost
    }
}

fn factorial(n: usize) -> f32 {
    FACTORIALS.get(n).copied().unwrap_or(f32::INFINITY)
}

/// Converts a small index (at most the number of coefficients) to `f32`.
#[expect(
    clippy::cast_precision_loss,
    reason = "the index is at most MAX_POLY_COEFFS"
)]
fn index_as_f32(index: usize) -> f32 {
    index as f32
}

fn degree_of(coeffs: &[f32]) -> u8 {
    u8::try_from(coeffs.len().saturating_sub(1)).unwrap_or(u8::MAX)
}

/// Solves `ax + b = 0` where `a` is not zero.
fn solve_linear(a: f32, b: f32) -> Roots {
    if is_zero(a) {
        Roots::default()
    } else {
        Roots::new(&[-b / a])
    }
}

/// Solves `ax^2 + bx + c = 0` where `a` is not zero.
fn solve_quadratic(a: f32, b: f32, c: f32) -> Roots {
    if is_zero(a) {
        return solve_linear(b, c);
    }
    let d = b * b - 4.0 * a * c;
    if is_zero(d) {
        Roots::new(&[-b / (2.0 * a)])
    } else if d > 0.0 {
        let d = sqrtf(d);
        Roots::new(&[(-b - d) / (2.0 * a), (-b + d) / (2.0 * a)])
    } else {
        Roots::default()
    }
}

/// Solves `ax^3 + bx^2 + cx + d = 0` where `a` is not zero, with Cardano's
/// formula.
fn solve_cubic(a: f32, b: f32, c: f32, d: f32) -> Roots {
    if is_zero(a) {
        return solve_quadratic(b, c, d);
    }

    // The roots of the depressed cubic t^3 + pt + q = 0, where x = t + offset
    let p = (3.0 * a * c - b * b) / (3.0 * a * a);
    let q = (2.0 * b * b * b - 9.0 * a * b * c + 27.0 * a * a * d) / (27.0 * a * a * a);
    let delta = (q * q) / 4.0 + (p * p * p) / 27.0;
    let offset = -b / (3.0 * a);

    let mut roots = if delta.abs() < 1e-8 {
        let u = cbrtf(-q / 2.0);
        if u == 0.0 {
            // A triple root
            Roots::new(&[0.0])
        } else {
            // A simple and a double root
            Roots::new(&[2.0 * u, -u])
        }
    } else if delta > 0.0 {
        // One real and two complex roots
        let sqrt_delta = sqrtf(delta);
        Roots::new(&[cbrtf(-q / 2.0 + sqrt_delta) + cbrtf(-q / 2.0 - sqrt_delta)])
    } else {
        // Three distinct real roots. The C library takes the complex cube
        // roots u and v of -q/2 ± i sqrt(-delta); they are conjugates, so the
        // roots u + v and -(u + v)/2 ± i sqrt(3)/2 (u - v) are
        // 2 m cos(phi), 2 m cos(phi + 2pi/3) and 2 m cos(phi - 2pi/3), in this
        // order, where m = |u| and phi = arg(u).
        let m = sqrtf(-p / 3.0);
        let phi = atan2f(sqrtf(-delta), -q / 2.0) / 3.0;
        Roots::new(&[
            2.0 * m * cosf(phi),
            2.0 * m * cosf(phi + 2.0 * PI / 3.0),
            2.0 * m * cosf(phi - 2.0 * PI / 3.0),
        ])
    };

    for root in roots.values.iter_mut().take(roots.len) {
        *root += offset;
    }
    roots
}

fn touches_constant(constant: f32, value: f32) -> Option<f32> {
    (value == constant).then_some(0.0)
}

/// Where `ax + b` equals the value in `[0, 1]`, if anywhere.
fn touches_linear(a: f32, b: f32, value: f32) -> Option<f32> {
    if is_zero(a) {
        touches_constant(b, value)
    } else if (a > 0.0 && value >= b && value <= a + b) || (a < 0.0 && value >= a + b && value <= b)
    {
        Some((value - b) / a)
    } else {
        None
    }
}

/// A 4D polynomial along the X, Y, Z and yaw coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Poly4D {
    /// The polynomial along the X coordinate.
    pub x: Poly,
    /// The polynomial along the Y coordinate.
    pub y: Poly,
    /// The polynomial along the Z coordinate.
    pub z: Poly,
    /// The polynomial along the yaw coordinate.
    pub yaw: Poly,
}

impl Poly4D {
    /// A constant 4D polynomial.
    #[must_use]
    pub fn constant(value: Vector3WithYaw) -> Self {
        Self {
            x: Poly::constant(value.x),
            y: Poly::constant(value.y),
            z: Poly::constant(value.z),
            yaw: Poly::constant(value.yaw),
        }
    }

    /// The constant zero 4D polynomial.
    #[must_use]
    pub fn zero() -> Self {
        Self::constant(Vector3WithYaw::default())
    }

    /// Evaluates the polynomial along each coordinate.
    #[must_use]
    pub fn eval(&self, t: f32) -> Vector3WithYaw {
        Vector3WithYaw {
            x: self.x.eval(t),
            y: self.y.eval(t),
            z: self.z.eval(t),
            yaw: self.yaw.eval(t),
        }
    }

    /// Replaces the polynomial with its derivative.
    pub fn deriv(&mut self) {
        self.x.deriv();
        self.y.deriv();
        self.z.deriv();
        self.yaw.deriv();
    }

    /// Multiplies the polynomial by a factor.
    pub fn scale(&mut self, factor: f32) {
        self.x.scale(factor);
        self.y.scale(factor);
        self.z.scale(factor);
        self.yaw.scale(factor);
    }
}

#[cfg(test)]
mod tests {
    // Test vectors from test/unit/test_poly.c of the C library.
    use super::*;

    fn assert_close(expected: f32, actual: f32, tolerance: f32) {
        assert!(
            (expected - actual).abs() <= tolerance,
            "expected {expected}, got {actual}"
        );
    }

    fn sorted(roots: Roots) -> Roots {
        let mut roots = roots;
        let len = roots.len;
        roots.values[..len].sort_by(f32::total_cmp);
        roots
    }

    #[test]
    fn constant() {
        let poly = Poly::constant(3.0);
        assert_eq!(poly.degree(), 0);
        for t in [0.0, -2.0, 1.0] {
            assert_eq!(poly.eval(t), 3.0);
            assert_eq!(poly.eval_double(f64::from(t)), 3.0);
        }
        assert_eq!(Poly::zero().coeffs(), [0.0]);
        assert_eq!(Poly4D::zero().eval(5.0), Vector3WithYaw::default());
    }

    #[test]
    fn linear() {
        let poly = Poly::linear(5.0, 10.0, 20.0);
        assert_eq!(poly.degree(), 1);
        for (t, expected) in [
            (-1.0, 8.0),
            (0.0, 10.0),
            (1.0, 12.0),
            (5.0, 20.0),
            (6.0, 22.0),
        ] {
            assert_eq!(poly.eval(t), expected);
            assert_eq!(poly.eval_double(f64::from(t)), f64::from(expected));
        }
    }

    #[test]
    fn linear_with_small_durations() {
        let eps = f32::EPSILON;
        let poly = Poly::linear(eps, 10.0, 20.0);
        assert_eq!(poly.eval(0.0), 10.0);
        assert_eq!(poly.eval(eps), 20.0);

        let poly = Poly::linear(eps, 1.0, 1.0 + 2.0 * eps);
        assert_eq!(poly.eval(0.0), 1.0);
        assert_eq!(poly.eval(eps / 2.0), 1.0 + eps);
        assert_eq!(poly.eval(eps), 1.0 + 2.0 * eps);

        // Too short: the constant average
        let poly = Poly::linear(f32::MIN_POSITIVE, 1.0, 1.0 + 2.0 * eps);
        assert_eq!(poly.eval(0.0), 1.0 + eps);
        assert_eq!(poly.eval(f32::MIN_POSITIVE), 1.0 + eps);

        let poly = Poly::linear(eps / 2.0, 1.0, 1.0 + eps);
        assert_eq!(poly.eval(0.0), 1.0);
        assert_eq!(poly.eval(eps / 2.0), 1.0);
    }

    #[test]
    fn bezier() {
        let poly = Poly::bezier(10.0, &[]);
        assert_eq!(poly.eval(5.0), 0.0);

        let poly = Poly::cubic_bezier(4.0, 0.0, 0.0, 5.0, 5.0);
        assert_eq!(poly.degree(), 3);
        for (t, expected) in [
            (0.0, 0.0),
            (1.0, 0.781_25),
            (2.0, 2.5),
            (3.0, 4.218_75),
            (4.0, 5.0),
        ] {
            assert_eq!(poly.eval(t), expected);
        }

        let poly = Poly::cubic_bezier(4.0, 0.0, 5.0, 5.0, 0.0);
        for (t, expected) in [
            (0.0, 0.0),
            (1.0, 2.8125),
            (2.0, 3.75),
            (3.0, 2.8125),
            (4.0, 0.0),
        ] {
            assert_eq!(poly.eval(t), expected);
            assert_eq!(poly.eval_double(f64::from(t)), f64::from(expected));
        }
    }

    #[test]
    fn add_constant_and_scale() {
        let mut poly = Poly::new(&[]);
        poly.add_constant(7.0);
        assert_eq!(poly.eval(3.0), 7.0);

        let mut poly = Poly::constant(50.0);
        poly.add_constant(7.0);
        assert_eq!(poly.eval(-1.0), 57.0);

        let mut poly = Poly::linear(5.0, 10.0, 20.0);
        poly.add_constant(3.0);
        assert_eq!(poly.eval(-1.0), 11.0);
        assert_eq!(poly.eval(5.0), 23.0);

        let mut poly = Poly::linear(5.0, 10.0, 20.0);
        poly.scale(3.0);
        assert_eq!(poly.eval(-1.0), 24.0);
        assert_eq!(poly.eval(5.0), 60.0);
    }

    #[test]
    fn deriv() {
        let mut poly = Poly::new(&[0.0, 7.0, 13.0, 61.0]);
        assert_eq!(poly.degree(), 3);
        for expected in [
            &[7.0, 26.0, 183.0][..],
            &[26.0, 366.0],
            &[366.0],
            &[0.0],
            &[0.0],
        ] {
            poly.deriv();
            assert_eq!(poly.coeffs(), expected);
            assert_eq!(poly.degree(), expected.len() - 1);
        }
    }

    #[test]
    fn stretch() {
        let poly = Poly::bezier(10.0, &[0.0, 7.0, 13.0, 61.0]);
        let mut stretched = poly;
        stretched.stretch(5.0);
        for i in 0..=10_u8 {
            let t = f32::from(i);
            assert_close(poly.eval(t), stretched.eval(t * 5.0), 1e-3);
        }
    }

    #[test]
    fn extrema() {
        let interval = |min, max| Ok(Interval { min, max });
        assert_eq!(Poly::new(&[]).extrema(), interval(0.0, 0.0));
        assert_eq!(Poly::constant(2.0).extrema(), interval(2.0, 2.0));
        assert_eq!(
            Poly::linear(5.0, 10.0, 20.0).extrema(),
            interval(10.0, 12.0)
        );
        assert_eq!(
            Poly::linear(5.0, 20.0, 10.0).extrema(),
            interval(18.0, 20.0)
        );
        assert_eq!(
            Poly::linear(5.0, 15.0, 15.0).extrema(),
            interval(15.0, 15.0)
        );
        assert_eq!(
            Poly::new(&[8.0, 2.0, 0.0, 0.0, 0.0]).extrema(),
            interval(8.0, 10.0)
        );
        assert_eq!(Poly::new(&[7.0, -4.0, 1.0]).extrema(), interval(4.0, 7.0));
        assert_eq!(
            Poly::new(&[41.0 / 16.0, -1.5, 1.0]).extrema(),
            interval(2.0, 41.0 / 16.0)
        );
        let concave = Poly::new(&[63.0 / 16.0, 0.5, -1.0]).extrema().unwrap();
        assert_close(55.0 / 16.0, concave.min, 1e-5);
        assert_eq!(concave.max, 4.0);
    }

    #[test]
    fn extrema_of_high_degrees_are_not_implemented() {
        // The C library returns success without a result here
        assert_eq!(
            Poly::new(&[1.0, 2.0, 3.0, 4.0, 5.0]).extrema(),
            Err(PolyError::UnsupportedDegree(4))
        );
    }

    #[test]
    fn solve_low_degrees() {
        let solve = |coeffs: &[f32], rhs| sorted(Poly::new(coeffs).solve(rhs).unwrap());
        assert!(solve(&[], 0.0).is_empty());
        assert!(solve(&[], 2.0).is_empty());
        assert_eq!(*solve(&[0.0], 0.0), [0.0]);
        assert!(solve(&[2.0], 0.0).is_empty());
        assert_eq!(*solve(&[2.0], 2.0), [0.0]);

        let linear = Poly::linear(5.0, 10.0, 20.0);
        assert_eq!(*linear.solve(0.0).unwrap(), [-5.0]);
        assert_eq!(*linear.solve(15.0).unwrap(), [2.5]);
        assert!(Poly::linear(5.0, 10.0, 10.0).solve(0.0).unwrap().is_empty());
        assert_eq!(*Poly::linear(5.0, 10.0, 10.0).solve(10.0).unwrap(), [0.0]);

        assert_eq!(*solve(&[10.0, 2.0, 0.0], 0.0), [-5.0]);
        assert_eq!(*solve(&[0.0, 2.0, 0.0], -10.0), [-5.0]);
        assert_eq!(*solve(&[9.0, -6.0, 1.0], 0.0), [3.0]);
        assert_eq!(*solve(&[9.0, -6.0, 1.0], 4.0), [1.0, 5.0]);
        assert_eq!(*solve(&[5.0, -6.0, 1.0], 0.0), [1.0, 5.0]);
        assert_eq!(*solve(&[5.0, -6.0, 1.0], -4.0), [3.0]);
        assert_eq!(*solve(&[6.0, -3.0, 0.0], 0.0), [2.0]);
        let roots = solve(&[2.0, -1.0, -3.0], 0.0);
        assert_eq!(roots.len(), 2);
        assert_close(-1.0, roots[0], 1e-7);
        assert_close(2.0 / 3.0, roots[1], 1e-7);
    }

    #[test]
    fn solve_cubics() {
        let check = |coeffs: &[f32], rhs, expected: &[f32], tolerance| {
            let roots = sorted(Poly::new(coeffs).solve(rhs).unwrap());
            assert_eq!(
                roots.len(),
                expected.len(),
                "roots of {coeffs:?}: {roots:?}"
            );
            for (&expected, &root) in expected.iter().zip(roots.iter()) {
                assert_close(expected, root, tolerance);
            }
        };
        // (x - 1)^3 = 4
        check(&[-5.0, 3.0, -3.0, 1.0], 0.0, &[1.0 + cbrtf(4.0)], 1e-5);
        check(&[5.0, -6.0, 1.0, 0.0], 0.0, &[1.0, 5.0], 1e-7);
        // Triple root
        check(&[-27.0, 27.0, -9.0, 1.0], 0.0, &[3.0], 1e-7);
        // Simple and double root
        check(&[-45.0, 39.0, -11.0, 1.0], 0.0, &[3.0, 5.0], 1e-4);
        // Three distinct real roots
        check(&[-15.0, 23.0, -9.0, 1.0], 0.0, &[1.0, 3.0, 5.0], 1e-4);
        check(&[0.0, 23.0, -9.0, 1.0], 15.0, &[1.0, 3.0, 5.0], 1e-4);
        // Casus irreducibilis of Cardano's formula
        check(
            &[3.0, -6.0, -9.0, 2.0],
            0.0,
            &[-0.876_36, 0.339_843, 5.036_517],
            1e-4,
        );
        // Through (0, 5), (0.25, 6), (0.75, 3) and (1, 7)
        check(
            &[5.0, 46.0 / 3.0, -56.0, 128.0 / 3.0],
            2.0,
            &[-0.128_953],
            1e-4,
        );
    }

    #[test]
    fn cubic_roots_match_the_c_library() {
        // Printed by sb_poly_solve() of the C library 6.0.0, in its order
        let cases: [(&[f32], f32, &[f32]); 4] = [
            (&[-15.0, 23.0, -9.0, 1.0], 0.0, &[5.0, 1.0, 3.0]),
            (
                &[3.0, -6.0, -9.0, 2.0],
                0.0,
                &[5.036_517, -0.876_359_9, 0.339_843],
            ),
            (&[0.0, 23.0, -9.0, 1.0], 15.0, &[5.0, 1.0, 3.0]),
            (
                &[0.1, -1.3, 0.2, 2.5],
                0.0,
                &[0.637_379_6, -0.796_200_2, 0.078_820_59],
            ),
        ];
        for (coeffs, rhs, expected) in cases {
            let roots = Poly::new(coeffs).solve(rhs).unwrap();
            assert_eq!(roots.len(), expected.len());
            for (&expected, &root) in expected.iter().zip(roots.iter()) {
                assert_close(expected, root, 1e-5);
            }
        }
    }

    #[test]
    fn three_real_roots_come_in_the_order_of_the_c_library() {
        // u + v first, then -(u + v)/2 + ..., then -(u + v)/2 - ...: for
        // (x - 1)(x - 3)(x - 5), these are 5, 1 and 3
        let roots = Poly::new(&[-15.0, 23.0, -9.0, 1.0]).solve(0.0).unwrap();
        for (expected, root) in [5.0, 1.0, 3.0].into_iter().zip(roots.iter()) {
            assert_close(expected, *root, 1e-4);
        }
    }

    #[test]
    fn solve_high_degrees_is_not_implemented() {
        assert_eq!(
            Poly::new(&[1.0, 0.0, 0.0, 0.0, 1.0]).solve(0.0),
            Err(PolyError::UnsupportedDegree(4))
        );
    }

    #[test]
    fn touches() {
        let touches = |coeffs: &[f32], value| Poly::new(coeffs).touches(value);

        assert_eq!(touches(&[], 0.0), Some(0.0));
        assert_eq!(touches(&[], 1.0), None);

        assert_eq!(touches(&[12.0], 0.0), None);
        assert_eq!(touches(&[12.0], 12.0), Some(0.0));

        // Linear from 1 to 3, and from 3 to 1
        assert_eq!(touches(&[1.0, 2.0], 0.0), None);
        assert_eq!(touches(&[1.0, 2.0], 2.0), Some(0.5));
        assert_eq!(touches(&[1.0, 2.0], 3.0), Some(1.0));
        assert_eq!(touches(&[3.0, -2.0], 1.0), Some(1.0));
        assert_eq!(touches(&[3.0, -2.0], 3.0), Some(0.0));
        assert_eq!(touches(&[3.0, -2.0], 4.0), None);

        // Convex quadratic rising from 1 to 6
        let convex = [1.0, 2.0, 3.0];
        assert_eq!(touches(&convex, 0.0), None);
        assert_eq!(touches(&convex, 1.0), Some(0.0));
        for (value, expected) in [
            (2.0, 1.0 / 3.0),
            (3.0, 0.548_583),
            (4.0, 0.720_759),
            (5.0, 0.868_517),
        ] {
            assert_close(expected, touches(&convex, value).unwrap(), 1e-4);
        }
        assert_eq!(touches(&convex, 6.0), Some(1.0));
        assert_eq!(touches(&convex, 7.0), None);

        // Concave quadratic falling from 2 to -2
        let concave = [2.0, -1.0, -3.0];
        assert_eq!(touches(&concave, 4.0), None);
        assert_close(2.0 / 3.0, touches(&concave, 0.0).unwrap(), 1e-6);
        assert_eq!(touches(&concave, -2.0), Some(1.0));
        assert_eq!(touches(&concave, -4.0), None);

        // Concave quadratic rising from 1 to 5 and back to 1: leftmost point
        let arch = [1.0, 16.0, -16.0];
        assert_eq!(touches(&arch, -1.0), None);
        assert_eq!(touches(&arch, 4.0), Some(0.25));
        assert_eq!(touches(&arch, 5.0), Some(0.5));
        assert_eq!(touches(&arch, 7.0), None);

        // Monotonic cubics without critical points in [0, 1]
        let rising = [1.0, 3.0, 3.0, 1.0];
        assert_eq!(touches(&rising, 0.0), None);
        assert_eq!(touches(&rising, 0.9999), None);
        assert_eq!(touches(&rising, 1.0), Some(0.0));
        assert_close(0.5, touches(&rising, 3.375).unwrap(), 1e-6);
        assert_eq!(touches(&rising, 8.0), Some(1.0));
        assert_eq!(touches(&rising, 8.00001), None);
        let falling = [1.0, -3.0, -3.0, -1.0];
        assert_eq!(touches(&falling, 1.00001), None);
        assert_close(0.5, touches(&falling, -1.375).unwrap(), 1e-6);
        assert_eq!(touches(&falling, -6.0), Some(1.0));
        assert_eq!(touches(&falling, -6.00001), None);

        // Not implemented above degree 3
        assert_eq!(touches(&[0.0, 0.0, 0.0, 0.0, 1.0], 0.0), None);
    }
}
