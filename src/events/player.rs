//! Playing an event list: firing its events in order as the time moves
//! forward.

use core::ops::Deref;

use super::{Event, EventList};

/// The largest time, in seconds, that is converted to milliseconds; later
/// times are clamped to `u32::MAX` milliseconds.
#[expect(
    clippy::cast_precision_loss,
    reason = "the limit is compared with a float, as in the C library"
)]
const MAX_TIME_SEC: f32 = (u32::MAX / 1000) as f32;

/// Plays an event list: it remembers the next event to fire, so that the
/// events can be fired in order as the time moves forward. The event list
/// must be sorted by timestamp.
///
/// The player holds the event list through any pointer to it, e.g. a
/// reference or an `Rc`.
#[derive(Clone, Debug)]
pub struct EventListPlayer<T> {
    events: T,
    current_index: usize,
}

impl<T: Deref<Target = EventList>> EventListPlayer<T> {
    /// A player of the given event list, at its start.
    pub const fn new(events: T) -> Self {
        Self {
            events,
            current_index: 0,
        }
    }

    /// The event list being played.
    pub fn events(&self) -> &EventList {
        &self.events
    }

    /// The index of the next event to fire in the event list. It is the
    /// length of the list if every event has been fired.
    pub const fn current_index(&self) -> usize {
        self.current_index
    }

    /// The next event to fire, without firing it, or `None` if every event
    /// has been fired.
    pub fn peek_next_event(&self) -> Option<&Event> {
        self.events.get(self.current_index)
    }

    /// Fires the next event and returns it, or returns `None` if every event
    /// has been fired.
    pub fn next_event(&mut self) -> Option<&Event> {
        let event = self.events.get(self.current_index)?;
        self.current_index += 1;
        Some(event)
    }

    /// Fires the next event and returns it if it is not later than the given
    /// time, in seconds. Returns `None` otherwise, and for negative times and
    /// NaN. Call it repeatedly to fire every event up to the given time.
    pub fn next_event_not_later_than(&mut self, time_sec: f32) -> Option<&Event> {
        if time_sec.is_nan() || time_sec < 0.0 {
            return None;
        }
        let time_msec = sec_to_msec(time_sec);
        let event = self
            .events
            .get(self.current_index)
            .filter(|event| event.time_msec <= time_msec)?;
        self.current_index += 1;
        Some(event)
    }

    /// Moves the player to the start of the event list.
    pub const fn rewind(&mut self) {
        self.current_index = 0;
    }

    /// Moves the player to the given time, in seconds: the next event to fire
    /// is the first one that is not earlier than the given time. Negative
    /// times and NaN move the player to the start of the event list.
    pub fn seek(&mut self, time_sec: f32) {
        self.current_index = 0;
        if time_sec.is_nan() || time_sec <= 0.0 {
            return;
        }
        let time_msec = sec_to_msec(time_sec);
        while self
            .events
            .get(self.current_index)
            .is_some_and(|event| event.time_msec < time_msec)
        {
            self.current_index += 1;
        }
    }
}

/// Converts a non-negative time in seconds to milliseconds, clamped to the
/// range of `u32`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the time is non-negative and below the limit, and is truncated as in the C library"
)]
fn sec_to_msec(time_sec: f32) -> u32 {
    if time_sec > MAX_TIME_SEC {
        u32::MAX
    } else {
        (time_sec * 1000.0) as u32
    }
}
