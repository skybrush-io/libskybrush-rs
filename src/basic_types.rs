//! Basic data types used throughout the library.

use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// Lower and upper limits of some quantity, both inclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Interval {
    /// The lower limit, inclusive.
    pub min: f32,
    /// The upper limit, inclusive.
    pub max: f32,
}

/// A 2D vector.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector2 {
    /// The X coordinate.
    pub x: f32,
    /// The Y coordinate.
    pub y: f32,
}

/// A 2D vector with signed 16-bit integer components.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Vector2I16 {
    /// The X coordinate.
    pub x: i16,
    /// The Y coordinate.
    pub y: i16,
}

/// A 3D vector.
///
/// Vectors can be added and subtracted, and multiplied and divided by
/// scalars, component by component.
///
/// # Examples
///
/// ```
/// use skybrush::basic_types::Vector3;
///
/// let start = Vector3 { x: 1.0, y: 2.0, z: 3.0 };
/// let velocity = Vector3 { x: 2.0, y: 0.0, z: -1.0 };
/// assert_eq!(start + velocity * 0.5, Vector3 { x: 2.0, y: 2.0, z: 2.5 });
/// assert_eq!((start - velocity) / 2.0, Vector3 { x: -0.5, y: 1.0, z: 2.0 });
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3 {
    /// The X coordinate.
    pub x: f32,
    /// The Y coordinate.
    pub y: f32,
    /// The Z coordinate.
    pub z: f32,
}

impl Vector3 {
    /// The zero vector.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// The vector with the given yaw angle.
    #[must_use]
    pub const fn with_yaw(self, yaw: f32) -> Vector3WithYaw {
        Vector3WithYaw {
            x: self.x,
            y: self.y,
            z: self.z,
            yaw,
        }
    }
}

/// A 3D vector with a yaw angle.
///
/// Like [`Vector3`], these can be added and subtracted, and multiplied and
/// divided by scalars, component by component; the yaw is one of the
/// components. For an operation on the coordinates only, use
/// [`xyz`](Self::xyz) and [`Vector3::with_yaw`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3WithYaw {
    /// The X coordinate.
    pub x: f32,
    /// The Y coordinate.
    pub y: f32,
    /// The Z coordinate.
    pub z: f32,
    /// The yaw angle.
    pub yaw: f32,
}

impl Vector3WithYaw {
    /// The vector without the yaw.
    #[must_use]
    pub const fn xyz(self) -> Vector3 {
        Vector3 {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

/// Implements the component-wise arithmetic operators for a vector type
/// with the given fields.
macro_rules! impl_vector_ops {
    ($type:ident { $($field:ident),+ }) => {
        impl Add for $type {
            type Output = Self;

            fn add(self, rhs: Self) -> Self {
                Self { $($field: self.$field + rhs.$field),+ }
            }
        }

        impl Sub for $type {
            type Output = Self;

            fn sub(self, rhs: Self) -> Self {
                Self { $($field: self.$field - rhs.$field),+ }
            }
        }

        impl Neg for $type {
            type Output = Self;

            fn neg(self) -> Self {
                Self { $($field: -self.$field),+ }
            }
        }

        impl Mul<f32> for $type {
            type Output = Self;

            fn mul(self, rhs: f32) -> Self {
                Self { $($field: self.$field * rhs),+ }
            }
        }

        impl Mul<$type> for f32 {
            type Output = $type;

            fn mul(self, rhs: $type) -> $type {
                rhs * self
            }
        }

        impl Div<f32> for $type {
            type Output = Self;

            fn div(self, rhs: f32) -> Self {
                Self { $($field: self.$field / rhs),+ }
            }
        }

        impl AddAssign for $type {
            fn add_assign(&mut self, rhs: Self) {
                *self = *self + rhs;
            }
        }

        impl SubAssign for $type {
            fn sub_assign(&mut self, rhs: Self) {
                *self = *self - rhs;
            }
        }

        impl MulAssign<f32> for $type {
            fn mul_assign(&mut self, rhs: f32) {
                *self = *self * rhs;
            }
        }

        impl DivAssign<f32> for $type {
            fn div_assign(&mut self, rhs: f32) {
                *self = *self / rhs;
            }
        }
    };
}

impl_vector_ops!(Vector3 { x, y, z });
impl_vector_ops!(Vector3WithYaw { x, y, z, yaw });

/// The bounding box of a set of 3D points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoundingBox {
    /// The minimum and maximum X coordinates.
    pub x: Interval,
    /// The minimum and maximum Y coordinates.
    pub y: Interval,
    /// The minimum and maximum Z coordinates.
    pub z: Interval,
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Vector3WithYaw = Vector3WithYaw {
        x: 1.0,
        y: -2.0,
        z: 4.0,
        yaw: 90.0,
    };
    const B: Vector3WithYaw = Vector3WithYaw {
        x: 0.5,
        y: 3.0,
        z: -1.0,
        yaw: -30.0,
    };

    #[test]
    fn operators_work_component_by_component() {
        let sum = Vector3WithYaw {
            x: 1.5,
            y: 1.0,
            z: 3.0,
            yaw: 60.0,
        };
        assert_eq!(A + B, sum);
        assert_eq!(sum - B, A);
        assert_eq!(-A + A, Vector3WithYaw::default());
        assert_eq!(A * 2.0, A + A);
        assert_eq!(2.0 * A, A * 2.0);
        assert_eq!(A * 2.0 / 2.0, A);
        assert_eq!((A + B).xyz(), A.xyz() + B.xyz());
        assert_eq!((A * 3.0).xyz(), 3.0 * A.xyz());
    }

    #[test]
    fn compound_assignments_match_the_operators() {
        let mut v = A;
        v += B;
        assert_eq!(v, A + B);
        v -= B;
        assert_eq!(v, A);
        v *= 4.0;
        assert_eq!(v, A * 4.0);
        v /= 4.0;
        assert_eq!(v, A);

        let mut w = Vector3::ZERO;
        w += A.xyz();
        w *= -1.0;
        assert_eq!(w, -A.xyz());
    }
}
