//! Playing a light program: executing its bytecode to find the color at given
//! time instants.
//!
//! The player follows the bytecode player of the C library closely, since the
//! oracle records the time instants where the player wakes up, not only the
//! colors. Times are milliseconds in `u64`, so that waits of up to about
//! 8.6 × 10^10 ms do not wrap around; the C library computes them in an
//! `unsigned long`.

use core::ops::Deref;

use super::{LightProgram, opcodes};
use crate::colors::RgbColor;

/// The deepest nesting of loops that a light program may use.
const MAX_LOOP_DEPTH: usize = 4;

/// How long the executor sleeps after the end of the light program, in
/// milliseconds.
const SLEEP_AFTER_END_MSEC: u64 = 60_000;

/// The length of a frame in the durations of the bytecode, in milliseconds.
const FRAME_MSEC: u64 = 20;

/// Plays a light program. It executes the bytecode one command at a time,
/// with fixed-size state, so it never allocates.
///
/// The player holds the light program through any pointer to it, e.g. a
/// reference or an `Rc`.
#[derive(Clone, Debug)]
pub struct LightPlayer<T> {
    program: T,
    executor: Executor,
    /// The time instant that the player was seeked to.
    current_timestamp: u64,
    /// The next time instant where the color may change.
    next_timestamp: u64,
}

impl<'a, T: Deref<Target = LightProgram<'a>>> LightPlayer<T> {
    /// A player of the given light program, at its start.
    pub fn new(program: T) -> Self {
        let mut executor = Executor::default();
        // Establish the origin of the internal clock of the executor at zero,
        // as the C library does before it attaches the bytecode
        executor.step(&[], 0);
        executor.rewind(program.bytes());
        Self {
            program,
            executor,
            current_timestamp: 0,
            next_timestamp: 0,
        }
    }

    /// The light program being played.
    pub fn program(&self) -> &LightProgram<'a> {
        &self.program
    }

    /// The color at the given time, in milliseconds.
    pub fn color_at(&mut self, timestamp: u64) -> RgbColor {
        self.seek(timestamp);
        self.executor.current_color
    }

    /// Moves the player to the given time, in milliseconds. Returns whether
    /// the light program has ended by then.
    ///
    /// Afterwards, [`next_timestamp`](Self::next_timestamp) is the next time
    /// instant where the color may change.
    pub fn seek(&mut self, timestamp: u64) -> bool {
        let bytes = self.program.bytes();
        if timestamp < self.current_timestamp {
            self.executor.rewind(bytes);
            self.current_timestamp = 0;
            self.next_timestamp = 0;
        }

        while timestamp > self.next_timestamp {
            // The target is after the next command, so step to it
            let mut proposal = self.executor.step(bytes, self.next_timestamp);
            // The executor never proposes a wakeup time before the time it
            // was stepped at; this keeps the loop going if it did
            if proposal < self.next_timestamp {
                proposal = self.next_timestamp.saturating_add(1);
            }
            self.current_timestamp = self.next_timestamp;
            self.next_timestamp = proposal;
        }

        // Evaluate the executor exactly at the target, as long as it wants to
        // wake up again by then: several commands that take no time may be
        // due at the target. This terminates because loops whose iterations
        // take no time cannot repeat forever.
        loop {
            self.next_timestamp = self.executor.step(bytes, timestamp);
            if self.next_timestamp > timestamp {
                break;
            }
        }
        self.current_timestamp = timestamp;
        self.executor.ended
    }

    /// The next time instant where the color may change after the time that
    /// the player was moved to, in milliseconds.
    #[must_use]
    pub const fn next_timestamp(&self) -> u64 {
        self.next_timestamp
    }
}

/// Executes the bytecode of a light program, as the command executor of the C
/// library.
#[derive(Clone, Debug)]
struct Executor {
    /// The offset of the next byte of the bytecode.
    position: usize,
    ended: bool,
    loops: LoopStack,
    /// The sum of the durations of the commands since the clock was reset,
    /// on the internal clock.
    cumulative_duration: u64,
    /// The time when the current command started.
    command_start_time: u64,
    /// The time when the internal clock was reset.
    clock_reset_time: u64,
    /// The time when the next command is due.
    next_wakeup_time: u64,
    /// Whether the internal clock is reset at the next step.
    reset_clock: bool,
    transition: Transition,
    current_color: RgbColor,
}

impl Default for Executor {
    fn default() -> Self {
        Self {
            position: 0,
            ended: true,
            loops: LoopStack::default(),
            cumulative_duration: 0,
            command_start_time: 0,
            clock_reset_time: 0,
            next_wakeup_time: 0,
            reset_clock: true,
            transition: Transition::default(),
            current_color: RgbColor::BLACK,
        }
    }
}

impl Executor {
    /// Rewinds the executor to the start of the bytecode. The clock is reset
    /// at the next step.
    fn rewind(&mut self, bytes: &[u8]) {
        self.position = 0;
        self.ended = bytes.is_empty();
        self.loops = LoopStack::default();
        self.transition.active = false;
        self.current_color = RgbColor::BLACK;
        self.reset_clock = true;
    }

    /// Advances the executor to the given time and returns the time when it
    /// wants to be stepped next.
    fn step(&mut self, bytes: &[u8], now: u64) -> u64 {
        if self.reset_clock {
            self.clock_reset_time = now;
            self.cumulative_duration = 0;
            self.set_color_and_reset_transition(RgbColor::BLACK);
            self.reset_clock = false;
            self.next_wakeup_time = now;
        }

        if self.ended {
            // Nothing will happen any more
            self.next_wakeup_time = now.wrapping_add(SLEEP_AFTER_END_MSEC);
            return self.next_wakeup_time;
        }

        if self.transition.active {
            self.step_transition(now);
        }

        if now >= self.next_wakeup_time {
            self.command_start_time = now;
            self.execute_next_command(bytes);
        }

        self.next_wakeup_time
    }

    fn execute_next_command(&mut self, bytes: &[u8]) {
        match self.next_byte(bytes) {
            opcodes::NOP => {}
            opcodes::SLEEP => {
                self.handle_delay(bytes);
            }
            opcodes::WAIT_UNTIL => {
                let deadline = u64::from(self.next_varint(bytes)) * FRAME_MSEC;
                self.delay_until(deadline);
                self.cumulative_duration =
                    self.next_wakeup_time.wrapping_sub(self.clock_reset_time);
            }
            opcodes::SET_COLOR => {
                let color = self.next_color(bytes);
                self.handle_delay(bytes);
                self.set_color_and_reset_transition(color);
            }
            opcodes::SET_GRAY => {
                let color = self.next_gray(bytes);
                self.handle_delay(bytes);
                self.set_color_and_reset_transition(color);
            }
            opcodes::SET_BLACK => {
                self.handle_delay(bytes);
                self.set_color_and_reset_transition(RgbColor::BLACK);
            }
            opcodes::SET_WHITE => {
                self.handle_delay(bytes);
                self.set_color_and_reset_transition(RgbColor::WHITE);
            }
            opcodes::FADE_TO_COLOR => {
                let color = self.next_color(bytes);
                self.fade_to(bytes, color);
            }
            opcodes::FADE_TO_GRAY => {
                let color = self.next_gray(bytes);
                self.fade_to(bytes, color);
            }
            opcodes::FADE_TO_BLACK => self.fade_to(bytes, RgbColor::BLACK),
            opcodes::FADE_TO_WHITE => self.fade_to(bytes, RgbColor::WHITE),
            opcodes::LOOP_BEGIN => {
                let iterations = self.next_byte(bytes);
                if !self
                    .loops
                    .begin(self.position, iterations, self.cumulative_duration)
                {
                    // Nested too deep; stop instead of executing the program
                    // incorrectly
                    self.ended = true;
                }
            }
            opcodes::LOOP_END => {
                if self
                    .loops
                    .current_iteration_has_no_duration(self.cumulative_duration)
                {
                    // An iteration that takes no time sets the same colors at
                    // the same instant as the next one would, so the rest of
                    // the iterations are skipped. Repeating them could keep
                    // the executor busy for a long time with nested loops, or
                    // forever with an infinite loop, which is rejected.
                    if self.loops.innermost_is_infinite() {
                        self.ended = true;
                    } else {
                        self.loops.leave();
                    }
                } else if let Some(start) = self.loops.end(self.cumulative_duration) {
                    self.position = start;
                }
            }
            // END, and unknown opcodes, stop the execution
            _ => self.ended = true,
        }
    }

    /// The next byte of the bytecode; the end of the bytecode reads as END.
    fn next_byte(&mut self, bytes: &[u8]) -> u8 {
        match bytes.get(self.position) {
            Some(&byte) => {
                self.position += 1;
                byte
            }
            None => opcodes::END,
        }
    }

    fn next_color(&mut self, bytes: &[u8]) -> RgbColor {
        let red = self.next_byte(bytes);
        let green = self.next_byte(bytes);
        let blue = self.next_byte(bytes);
        RgbColor::new(red, green, blue)
    }

    fn next_gray(&mut self, bytes: &[u8]) -> RgbColor {
        let value = self.next_byte(bytes);
        RgbColor::new(value, value, value)
    }

    /// The next varint of the bytecode. Varints hold at most 32 bits; larger
    /// values are clamped to `u32::MAX`, and the rest of the varint is
    /// skipped.
    fn next_varint(&mut self, bytes: &[u8]) -> u32 {
        let mut result: u32 = 0;
        let mut index: u32 = 0;
        let mut overflow = false;
        loop {
            let byte = self.next_byte(bytes);
            let value = u32::from(byte & 0x7f);
            if index < 4 || (index == 4 && value < 0x10) {
                result |= value << (7 * index);
                index += 1;
            } else {
                overflow = true;
            }
            if byte & 0x80 == 0 {
                break;
            }
        }
        if overflow { u32::MAX } else { result }
    }

    /// Reads a duration and delays the next command by it, on the internal
    /// clock.
    fn handle_delay(&mut self, bytes: &[u8]) {
        let duration = u64::from(self.next_varint(bytes)) * FRAME_MSEC;
        self.cumulative_duration = self.cumulative_duration.wrapping_add(duration);
        self.delay_until(self.cumulative_duration);
    }

    /// Delays the next command until the given time of the internal clock,
    /// unless it is delayed until later already.
    fn delay_until(&mut self, internal_time: u64) {
        let time = self.clock_reset_time.wrapping_add(internal_time);
        self.next_wakeup_time = self.next_wakeup_time.max(time);
    }

    /// Starts a fade from the current color to the given one, lasting until
    /// the next command.
    fn fade_to(&mut self, bytes: &[u8], color: RgbColor) {
        let now = self.command_start_time;
        self.handle_delay(bytes);
        let duration = self.next_wakeup_time.wrapping_sub(now);

        self.transition.end_color = color;
        self.transition.active = true;
        self.transition.start_time = self.command_start_time;
        self.transition.duration = duration;
        self.step_transition(now);
    }

    /// Updates the color of the active transition at the given time.
    fn step_transition(&mut self, now: u64) {
        let progress = self.transition.progress(now);
        self.current_color = self
            .transition
            .start_color
            .linear_interpolation(self.transition.end_color, progress);
        self.transition.active = progress < 1.0;
        if !self.transition.active {
            // The next transition starts from the end color of this one
            self.transition.start_color = self.transition.end_color;
        }
    }

    fn set_color_and_reset_transition(&mut self, color: RgbColor) {
        self.current_color = color;
        self.transition.start_color = color;
    }
}

/// A transition between two colors.
#[derive(Clone, Copy, Debug, Default)]
struct Transition {
    active: bool,
    start_time: u64,
    duration: u64,
    start_color: RgbColor,
    end_color: RgbColor,
}

impl Transition {
    /// The progress of the transition at the given time, from 0 to 1.
    #[expect(
        clippy::cast_precision_loss,
        reason = "the progress is a float, as in the C library"
    )]
    fn progress(&self, now: u64) -> f32 {
        if now < self.start_time {
            0.0
        } else if self.duration == 0 {
            1.0
        } else {
            let progress = (now - self.start_time) as f32 / self.duration as f32;
            progress.min(1.0)
        }
    }
}

/// The loops being executed, innermost last.
#[derive(Clone, Copy, Debug, Default)]
struct LoopStack {
    items: [LoopStackItem; MAX_LOOP_DEPTH],
    depth: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct LoopStackItem {
    /// The offset of the first command of the body of the loop.
    start: usize,
    /// The number of iterations left after the current one, plus one; zero
    /// for an infinite loop.
    iterations_left_plus_one: u8,
    /// The time of the internal clock when the current iteration started.
    iteration_start_time: u64,
}

impl LoopStack {
    /// Starts a loop with the given number of iterations (zero means
    /// infinite). Returns `false` if loops are nested too deep.
    fn begin(&mut self, start: usize, iterations: u8, time: u64) -> bool {
        let Some(item) = self.items.get_mut(self.depth) else {
            return false;
        };
        // An infinite loop stores zero. A finite loop would store the number
        // of iterations plus one, but the first iteration starts right away.
        *item = LoopStackItem {
            start,
            iterations_left_plus_one: iterations,
            iteration_start_time: time,
        };
        self.depth += 1;
        true
    }

    /// Ends the current iteration of the innermost loop at the given time of
    /// the internal clock. Returns the start of its body if it has iterations
    /// left.
    fn end(&mut self, time: u64) -> Option<usize> {
        let index = self.depth.checked_sub(1)?;
        let item = self.items.get_mut(index)?;
        match item.iterations_left_plus_one {
            // Infinite loop
            0 => {
                item.iteration_start_time = time;
                Some(item.start)
            }
            // Last iteration
            1 => {
                item.iterations_left_plus_one = 0;
                self.depth = index;
                None
            }
            _ => {
                item.iterations_left_plus_one -= 1;
                item.iteration_start_time = time;
                Some(item.start)
            }
        }
    }

    /// Leaves the innermost loop, skipping its remaining iterations.
    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn innermost(&self) -> Option<&LoopStackItem> {
        self.items.get(self.depth.checked_sub(1)?)
    }

    fn innermost_is_infinite(&self) -> bool {
        self.innermost()
            .is_some_and(|item| item.iterations_left_plus_one == 0)
    }

    fn current_iteration_has_no_duration(&self, time: u64) -> bool {
        self.innermost()
            .is_some_and(|item| item.iteration_start_time == time)
    }
}
