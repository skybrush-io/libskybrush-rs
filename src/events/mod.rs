//! Events placed on the timeline of a show, such as pyro triggers and light
//! control events, and the player that fires them as the time moves forward.
//!
//! An event list is copied out of the show file when it is loaded, since it is
//! usually modified afterwards (pyro off events are added, timestamps are
//! adjusted). Events are kept in increasing order of their timestamps.

mod player;

use alloc::collections::TryReserveError;
use alloc::vec::IntoIter;
use alloc::vec::Vec;
use core::slice::Iter;
use core::{error, fmt};

pub use player::EventListPlayer;

use crate::formats::binary::{BinaryBlockType, BinaryFile, BinaryFileError};

/// The size of an event in an event list block: a 32-bit timestamp, the type,
/// the subtype and a 4-byte payload.
const SIZE_OF_EVENT: usize = 10;

/// The payload of a pyro event that turns the channel off.
const PYRO_OFF_PAYLOAD: [u8; 4] = [0xff; 4];

/// The type of an event.
///
/// Types that the crate does not know are kept as opaque events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EventType(pub u8);

impl EventType {
    /// No event.
    pub const NONE: Self = Self(0);
    /// A pyro trigger event. The subtype is the index of the pyro channel; the
    /// payload is all ones to turn the channel off, and anything else to turn
    /// it on.
    pub const PYRO: Self = Self(1);
    /// A light control event; see the `LIGHT_` constants of [`Event`] for its
    /// subtypes.
    pub const LIGHT: Self = Self(2);
}

/// An event that is triggered at a time instant of the show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Event {
    /// The time when the event is triggered, in milliseconds.
    pub time_msec: u32,
    /// The type of the event.
    pub event_type: EventType,
    /// The subtype of the event; its meaning depends on the type.
    pub subtype: u8,
    /// The payload of the event; its meaning depends on the type and the
    /// subtype.
    pub payload: [u8; 4],
}

impl Event {
    /// The subtype of a light event that sets the source of a light output.
    /// The first byte of the payload is the index of the output, the second
    /// is the source (defined by the application), and the rest is a color
    /// in RGB565 for sources that need one.
    pub const LIGHT_SET_SOURCE: u8 = 0;
    /// The subtype of a light event that sets the X coordinate of the pixel
    /// that the drone represents in interactive control from the ground
    /// station, in the lowest 12 bits of the payload.
    pub const LIGHT_SET_PIXEL_X: u8 = 1;
    /// The subtype of a light event that sets the Y coordinate of the pixel
    /// that the drone represents, like [`LIGHT_SET_PIXEL_X`](Self::LIGHT_SET_PIXEL_X).
    pub const LIGHT_SET_PIXEL_Y: u8 = 2;

    /// The payload as a little-endian 32-bit unsigned integer.
    #[must_use]
    pub const fn payload_u32(&self) -> u32 {
        u32::from_le_bytes(self.payload)
    }

    /// The payload as a little-endian IEEE 754 float.
    #[must_use]
    pub const fn payload_f32(&self) -> f32 {
        f32::from_le_bytes(self.payload)
    }

    /// Whether the event turns a pyro channel on.
    #[must_use]
    pub fn is_pyro_on(&self) -> bool {
        self.event_type == EventType::PYRO && self.payload != PYRO_OFF_PAYLOAD
    }

    /// Whether the event turns a pyro channel off.
    #[must_use]
    pub fn is_pyro_off(&self) -> bool {
        self.event_type == EventType::PYRO && self.payload == PYRO_OFF_PAYLOAD
    }
}

/// Why an event list could not be read or modified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventListError {
    /// The events of the event list block are not sorted by timestamp.
    Corrupted,
    /// An event was appended with a timestamp earlier than that of the last
    /// event.
    OutOfOrder,
    /// The show file could not be read.
    File(BinaryFileError),
    /// Memory could not be allocated.
    OutOfMemory,
}

impl fmt::Display for EventListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupted => f.write_str("the events are not sorted by timestamp"),
            Self::OutOfOrder => f.write_str("the event is earlier than the last event"),
            Self::File(_) => f.write_str("the show file could not be read"),
            Self::OutOfMemory => f.write_str("out of memory"),
        }
    }
}

impl error::Error for EventListError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::File(err) => Some(err),
            _ => None,
        }
    }
}

impl From<BinaryFileError> for EventListError {
    fn from(err: BinaryFileError) -> Self {
        Self::File(err)
    }
}

impl From<TryReserveError> for EventListError {
    fn from(_: TryReserveError) -> Self {
        Self::OutOfMemory
    }
}

/// The events of a show, in increasing order of their timestamps.
///
/// The list does not implement `FromIterator` and `Extend`: they cannot
/// report that memory could not be allocated. Add events with
/// [`append`](Self::append) or [`insert`](Self::insert) instead, which do.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EventList {
    events: Vec<Event>,
}

impl EventList {
    /// An empty event list.
    #[must_use]
    pub const fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// The events encoded in the body of an event list block. Trailing bytes
    /// that do not form a complete event are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`EventListError::Corrupted`] if the events are not sorted by
    /// timestamp.
    pub fn from_buffer(body: &[u8]) -> Result<Self, EventListError> {
        let (encoded, _trailing) = body.as_chunks::<SIZE_OF_EVENT>();
        let mut events = Vec::new();
        events.try_reserve_exact(encoded.len())?;
        for &[t0, t1, t2, t3, event_type, subtype, p0, p1, p2, p3] in encoded {
            let event = Event {
                time_msec: u32::from_le_bytes([t0, t1, t2, t3]),
                event_type: EventType(event_type),
                subtype,
                payload: [p0, p1, p2, p3],
            };
            if events
                .last()
                .is_some_and(|last: &Event| event.time_msec < last.time_msec)
            {
                return Err(EventListError::Corrupted);
            }
            events.push(event);
        }
        Ok(Self { events })
    }

    /// The event list of a show file, or `None` if the show file has no event
    /// list.
    ///
    /// # Errors
    ///
    /// Returns an error if the event list block is truncated or its events
    /// are not sorted by timestamp.
    pub fn from_binary_file(file: &BinaryFile<'_>) -> Result<Option<Self>, EventListError> {
        file.find_first_block_by_type(BinaryBlockType::EVENT_LIST)?
            .map(|block| Self::from_buffer(block.body))
            .transpose()
    }

    /// The number of events in the list.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the list has no events.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The number of events that the list can hold without allocating.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.events.capacity()
    }

    /// The events of the list.
    #[must_use]
    pub fn as_slice(&self) -> &[Event] {
        &self.events
    }

    /// An iterator over the events of the list.
    pub fn iter(&self) -> Iter<'_, Event> {
        self.events.iter()
    }

    /// The event at the given index, or `None` if there is no such event.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Event> {
        self.events.get(index)
    }

    /// The event at the given index, or `None` if there is no such event.
    /// Changing its timestamp may make the list unsorted; see
    /// [`sort`](Self::sort).
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Event> {
        self.events.get_mut(index)
    }

    /// Removes all events from the list.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Appends an event to the end of the list.
    ///
    /// # Errors
    ///
    /// Returns [`EventListError::OutOfOrder`] if the event is earlier than
    /// the last event of the list, and [`EventListError::OutOfMemory`] if
    /// memory could not be allocated. The list is left intact then.
    pub fn append(&mut self, event: Event) -> Result<(), EventListError> {
        if self
            .events
            .last()
            .is_some_and(|last| event.time_msec < last.time_msec)
        {
            return Err(EventListError::OutOfOrder);
        }
        self.events.try_reserve(1)?;
        self.events.push(event);
        Ok(())
    }

    /// Inserts an event after the last event whose timestamp is not later
    /// than that of the event.
    ///
    /// # Errors
    ///
    /// Returns [`EventListError::OutOfMemory`] if memory could not be
    /// allocated; the list is left intact then.
    pub fn insert(&mut self, event: Event) -> Result<(), EventListError> {
        self.events.try_reserve(1)?;
        self.insert_reserved(event);
        Ok(())
    }

    /// Inserts an event, as [`insert`](Self::insert), into a list that has
    /// room for it.
    fn insert_reserved(&mut self, event: Event) {
        // Binary search for the first event that is later than the new one
        let (mut left, mut right) = (0, self.events.len());
        while left < right {
            let mid = left + (right - left) / 2;
            if self
                .events
                .get(mid)
                .is_some_and(|other| other.time_msec <= event.time_msec)
            {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        self.events.insert(left, event);
    }

    /// Removes the event at the given index and returns it, or returns `None`
    /// if there is no such event.
    pub fn remove(&mut self, index: usize) -> Option<Event> {
        (index < self.events.len()).then(|| self.events.remove(index))
    }

    /// Whether the events are sorted by timestamp.
    #[must_use]
    pub fn is_sorted(&self) -> bool {
        self.events.is_sorted_by_key(|event| event.time_msec)
    }

    /// Sorts the events by timestamp. Events with equal timestamps keep their
    /// order.
    pub fn sort(&mut self) {
        // An insertion sort: it is stable and needs no memory, and the lists
        // are short and usually almost sorted
        for i in 1..self.events.len() {
            let mut j = i;
            while j > 0 && self.time_msec_at(j - 1) > self.time_msec_at(j) {
                self.events.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    fn time_msec_at(&self, index: usize) -> u32 {
        self.events.get(index).map_or(0, |event| event.time_msec)
    }

    /// Makes every pyro ignition last at most the given duration, in
    /// milliseconds: an off event is added after every on event whose channel
    /// is not turned off by then. A later off event of the channel is removed
    /// then. Off events that would come after `u32::MAX` are placed there.
    ///
    /// # Errors
    ///
    /// Returns [`EventListError::OutOfMemory`] if memory could not be
    /// allocated; the list is left intact then.
    pub fn add_pyro_off_events(&mut self, duration_msec: u32) -> Result<(), EventListError> {
        // Every on event adds at most one off event; reserving the memory up
        // front makes the operation all-or-nothing
        let num_on_events = self
            .events
            .iter()
            .filter(|event| event.is_pyro_on())
            .count();
        self.events.try_reserve(num_on_events)?;

        let mut i = 0;
        while let Some(&event) = self.events.get(i) {
            i += 1;
            if !event.is_pyro_on() {
                continue;
            }
            let off_time_msec = event.time_msec.saturating_add(duration_msec);

            // The next off event of the same channel; later on events of the
            // channel do not end the search
            let off_event = self
                .events
                .iter()
                .enumerate()
                .skip(i)
                .find(|(_, other)| other.is_pyro_off() && other.subtype == event.subtype)
                .map(|(index, other)| (index, other.time_msec));
            match off_event {
                Some((_, time_msec)) if time_msec <= off_time_msec => {}
                found => {
                    if let Some((index, _)) = found {
                        self.events.remove(index);
                    }
                    // The off event comes after the on event, so the index
                    // of the next event stays valid
                    self.insert_reserved(Event {
                        time_msec: off_time_msec,
                        event_type: EventType::PYRO,
                        subtype: event.subtype,
                        payload: PYRO_OFF_PAYLOAD,
                    });
                }
            }
        }
        Ok(())
    }

    /// Moves the events of the given type by the given number of
    /// milliseconds, and sorts the list. Timestamps are clamped to the range
    /// of `u32`.
    pub fn adjust_timestamps_by_type(&mut self, event_type: EventType, delta_msec: i32) {
        for event in &mut self.events {
            if event.event_type == event_type {
                event.time_msec = event.time_msec.saturating_add_signed(delta_msec);
            }
        }
        self.sort();
    }
}

impl<'a> IntoIterator for &'a EventList {
    type Item = &'a Event;
    type IntoIter = Iter<'a, Event>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for EventList {
    type Item = Event;
    type IntoIter = IntoIter<Event>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pyro(time_msec: u32, channel: u8, on: bool) -> Event {
        Event {
            time_msec,
            event_type: EventType::PYRO,
            subtype: channel,
            payload: if on { [0; 4] } else { PYRO_OFF_PAYLOAD },
        }
    }

    #[test]
    fn sorting_keeps_the_order_of_equal_timestamps() {
        let mut list = EventList::new();
        for event in [
            pyro(3000, 1, true),
            pyro(1000, 2, true),
            pyro(1000, 3, true),
        ] {
            list.events.push(event);
        }
        list.sort();
        let channels: Vec<u8> = list.iter().map(|event| event.subtype).collect();
        assert_eq!(channels, [2, 3, 1]);
    }

    #[test]
    fn removing_a_late_off_event_adds_one_in_its_place() {
        let mut list = EventList::new();
        for event in [pyro(1000, 1, true), pyro(9000, 1, false)] {
            list.insert(event).unwrap_or_default();
        }
        assert_eq!(list.add_pyro_off_events(500), Ok(()));
        assert_eq!(list.as_slice(), [pyro(1000, 1, true), pyro(1500, 1, false)]);
    }
}
