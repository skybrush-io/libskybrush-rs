//! Basic data types used throughout the library.

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

    /// The vector multiplied by a scalar.
    #[must_use]
    pub const fn scaled(self, factor: f32) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
        }
    }
}

/// A 3D vector with a yaw angle.
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
