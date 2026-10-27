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
