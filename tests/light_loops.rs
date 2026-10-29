//! Checks that loops in light programs behave like their unrolled bodies.
//!
//! The light player skips the remaining iterations of a finite loop after an
//! iteration that takes no time. This must not change what the player
//! reports, so random programs with nested finite loops are played next to
//! the same programs with every loop unrolled, and the colors, the times of
//! the next changes and the ends must agree.
#![allow(clippy::indexing_slicing)]

use skybrush::colors::RgbColor;
use skybrush::lights::{LightPlayer, LightProgram};

const SLEEP: u8 = 0x02;
const WAIT_UNTIL: u8 = 0x03;
const SET_COLOR: u8 = 0x04;
const SET_GRAY: u8 = 0x05;
const SET_BLACK: u8 = 0x06;
const SET_WHITE: u8 = 0x07;
const FADE_TO_COLOR: u8 = 0x08;
const FADE_TO_GRAY: u8 = 0x09;
const FADE_TO_BLACK: u8 = 0x0a;
const FADE_TO_WHITE: u8 = 0x0b;
const LOOP_BEGIN: u8 = 0x0c;
const LOOP_END: u8 = 0x0d;

/// A xorshift generator, so that the test needs no dependencies and is
/// reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u8 {
        u8::try_from(self.next() % bound).unwrap_or_default()
    }
}

enum Node {
    Command(Vec<u8>),
    Loop(u8, Vec<Node>),
}

/// A duration in frames; most commands take no time, so that most iterations
/// take no time either.
fn duration(rng: &mut Rng) -> u8 {
    if rng.below(3) == 0 { rng.below(4) } else { 0 }
}

fn command(rng: &mut Rng) -> Node {
    let color = [rng.below(256), rng.below(256), rng.below(256)];
    let mut bytes = match rng.below(10) {
        0 => vec![SLEEP],
        // Deadlines in the past take no time
        1 => vec![WAIT_UNTIL, rng.below(30)],
        2 => [&[SET_COLOR][..], &color].concat(),
        3 => vec![SET_GRAY, color[0]],
        4 => vec![SET_BLACK],
        5 => vec![SET_WHITE],
        6 => [&[FADE_TO_COLOR][..], &color].concat(),
        7 => vec![FADE_TO_GRAY, color[0]],
        8 => vec![FADE_TO_BLACK],
        _ => vec![FADE_TO_WHITE],
    };
    if bytes[0] != WAIT_UNTIL {
        bytes.push(duration(rng));
    }
    Node::Command(bytes)
}

fn body(rng: &mut Rng, depth: usize) -> Vec<Node> {
    (0..=rng.below(4))
        .map(|_| {
            // Three levels of loops, below the limit of the nesting
            if depth < 3 && rng.below(3) == 0 {
                Node::Loop(1 + rng.below(4), body(rng, depth + 1))
            } else {
                command(rng)
            }
        })
        .collect()
}

fn encode(nodes: &[Node], unrolled: bool, out: &mut Vec<u8>) {
    for node in nodes {
        match node {
            Node::Command(bytes) => out.extend_from_slice(bytes),
            Node::Loop(iterations, body) if unrolled => {
                for _ in 0..*iterations {
                    encode(body, unrolled, out);
                }
            }
            Node::Loop(iterations, body) => {
                out.extend_from_slice(&[LOOP_BEGIN, *iterations]);
                encode(body, unrolled, out);
                out.push(LOOP_END);
            }
        }
    }
}

/// What the player reports at a time instant.
type Observation = (u64, RgbColor, u64, bool);

fn observe(bytes: Vec<u8>) -> Vec<Observation> {
    let program = LightProgram::from_vec(bytes);
    let mut player = LightPlayer::new(&program);
    let mut result = Vec::new();

    // Every change, until the end
    let mut t = 0;
    for _ in 0..10_000 {
        let observation = seek(&mut player, t);
        result.push(observation);
        if observation.3 {
            break;
        }
        t = player.next_timestamp();
    }

    // Samples in between, forwards and backwards
    for t in (0..=t).step_by(7) {
        result.push(seek(&mut player, t));
    }
    for t in (0..=t).rev().step_by(13) {
        result.push(seek(&mut player, t));
    }
    result
}

fn seek(player: &mut LightPlayer<&LightProgram<'_>>, t: u64) -> Observation {
    let ended = player.seek(t);
    let color = player.color_at(t);
    (t, color, player.next_timestamp(), ended)
}

#[test]
fn loops_behave_like_their_unrolled_bodies() {
    let mut rng = Rng(0x5eed_cafe_f00d_beef);
    for case in 0..2000 {
        let program = body(&mut rng, 0);
        let (mut looped, mut unrolled) = (Vec::new(), Vec::new());
        encode(&program, false, &mut looped);
        encode(&program, true, &mut unrolled);
        assert_eq!(
            observe(looped.clone()),
            observe(unrolled),
            "case {case}: {looped:02x?}"
        );
    }
}

#[test]
fn nested_loops_that_take_no_time_do_not_stall_the_player() {
    // Four levels of 255 iterations would execute the body about 4 × 10^9
    // times if every iteration were executed
    let mut bytes = Vec::new();
    for _ in 0..4 {
        bytes.extend_from_slice(&[LOOP_BEGIN, 255]);
    }
    bytes.extend_from_slice(&[SET_COLOR, 255, 0, 0, 0]);
    bytes.extend_from_slice(&[LOOP_END; 4]);
    bytes.extend_from_slice(&[SET_COLOR, 0, 255, 0, 50]);

    let program = LightProgram::from_vec(bytes);
    let mut player = LightPlayer::new(&program);
    assert_eq!(player.color_at(0), RgbColor::new(0, 255, 0));
    assert_eq!(player.next_timestamp(), 1000);
    assert!(player.seek(1000));
}
