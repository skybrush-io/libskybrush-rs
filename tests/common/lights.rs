//! Playing light programs like the generators of the oracle do.

use serde_json::{Value, json};
use skybrush::colors::RgbColor;
use skybrush::lights::{LightPlayer, LightProgram};

/// The time when a light program ends, scanning it up to the given time in
/// milliseconds as the generators do: seeking to each time instant where the
/// player wakes up next. `None` if it does not end by then.
pub fn end_msec(program: &LightProgram<'_>, limit_msec: u64) -> Option<u64> {
    let mut player = LightPlayer::new(program);
    let mut t = 0;
    while t <= limit_msec {
        if player.seek(t) {
            return Some(t);
        }
        let next = player.next_timestamp();
        t = if next > t { next } else { t + 1 };
    }
    None
}

/// A color, as `[red, green, blue]`.
pub fn color(color: RgbColor) -> Value {
    json!([color.red, color.green, color.blue])
}
