//! Motion helpers: travel times and velocities of simple motions, and cubic
//! Bézier curves with given velocities at their ends.

use libm::sqrtf;

use crate::basic_types::Vector3WithYaw;

/// The time needed to travel the given distance with a motion of three
/// phases: constant acceleration up to `max_speed`, constant speed, and
/// constant deceleration to a stop. The motion starts and ends at rest; if
/// the distance is short, it never reaches the maximum speed.
///
/// An infinite `acceleration` means constant speed during the entire motion.
/// Returns infinity if the distance is negative, or the speed or the
/// acceleration is not positive.
#[must_use]
pub fn travel_time_for_distance(distance: f32, max_speed: f32, acceleration: f32) -> f32 {
    if distance < 0.0 || max_speed <= 0.0 || acceleration <= 0.0 {
        return f32::INFINITY;
    }
    if distance == 0.0 {
        return 0.0;
    }
    if acceleration == f32::INFINITY {
        return distance / max_speed;
    }

    // The time and the distance of accelerating from zero to the maximum
    // speed
    let t1 = max_speed / acceleration;
    let s1 = acceleration / 2.0 * t1 * t1;
    if distance >= 2.0 * s1 {
        // Full acceleration, then constant speed for the rest
        let t2 = (distance - 2.0 * s1) / max_speed;
        2.0 * t1 + t2
    } else {
        // Accelerating for half of the distance, then decelerating
        2.0 * sqrtf(distance / acceleration)
    }
}

/// The maximum speed of a motion of three phases (as in
/// [`travel_time_for_distance`]) that travels the given distance in the given
/// time.
///
/// An infinite `acceleration` means constant speed during the entire motion.
/// Returns infinity if the distance is negative, the time or the acceleration
/// is not positive, or the distance cannot be traveled in the given time.
#[must_use]
pub fn travel_velocity_for_distance(distance: f32, time: f32, acceleration: f32) -> f32 {
    if distance < 0.0 || time <= 0.0 || acceleration <= 0.0 {
        return f32::INFINITY;
    }
    if distance == 0.0 {
        return 0.0;
    }
    if acceleration == f32::INFINITY {
        return distance / time;
    }

    let d = time * time - 4.0 * distance / acceleration;
    if d >= 0.0 {
        -(sqrtf(d) - time) / 2.0 * acceleration
    } else {
        f32::INFINITY
    }
}

/// The two inner control points of a cubic Bézier curve between `start` and
/// `end` that takes `duration_sec` seconds and has the given velocities at
/// its ends. The accelerations at the ends are zero.
///
/// Returns `None` if the duration is not positive.
#[must_use]
pub fn cubic_bezier_from_velocity_constraints(
    start: Vector3WithYaw,
    start_vel: Vector3WithYaw,
    end: Vector3WithYaw,
    end_vel: Vector3WithYaw,
    duration_sec: f32,
) -> Option<(Vector3WithYaw, Vector3WithYaw)> {
    if duration_sec <= 0.0 {
        return None;
    }

    let scale = duration_sec / 3.0;
    let control1 = Vector3WithYaw {
        x: start.x + scale * start_vel.x,
        y: start.y + scale * start_vel.y,
        z: start.z + scale * start_vel.z,
        yaw: start.yaw + scale * start_vel.yaw,
    };
    let control2 = Vector3WithYaw {
        x: end.x - scale * end_vel.x,
        y: end.y - scale * end_vel.y,
        z: end.z - scale * end_vel.z,
        yaw: end.yaw - scale * end_vel.yaw,
    };
    Some((control1, control2))
}
