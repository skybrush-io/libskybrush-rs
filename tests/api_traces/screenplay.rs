//! Replays the `screenplay_queries` section of `scenarios.json`: queries of
//! the scenes of a screenplay, and screenplays loaded from show files.
//!
//! The scenes are built as in `json_screenplay_queries()` of the generator;
//! the golden data records the results only.

use std::ptr;
use std::rc::Rc;

use serde_json::{Map, Value, json};
use skybrush::basic_types::Vector3WithYaw;
use skybrush::formats::binary::BinaryFile;
use skybrush::screenplay::{Screenplay, ScreenplayScene, ScreenplaySceneFlags};
use skybrush::time_axis::TimeSegment;
use skybrush::trajectory::{Trajectory, TrajectoryBuilder};

use crate::common::Replay;
use crate::common::errors::{ErrorName, result_name};
use crate::common::oracle::{decode_hex, encode_hex};

/// The warped times and the durations that the generator queries; the
/// oracle records the non-finite ones as `null`.
const WARPED_TIMES_SEC: [f32; 10] = [-1.0, 0.0, 0.5, 2.0, 3.9, 4.0, 5.0, 6.0, 100.0, f32::NAN];
const DURATIONS_SEC: [f32; 10] = [
    0.0,
    0.0004,
    0.0005,
    2.3456,
    -1.0,
    f32::NAN,
    f32::INFINITY,
    f32::NEG_INFINITY,
    4_294_967.0,
    4_294_967.5,
];

pub fn replay_queries(golden: &Value) -> Replay {
    let screenplay = screenplay().expect("valid screenplay");
    let mut other = Screenplay::new();
    let foreign = other.append_new_scene().expect("memory");

    let scenes: Vec<Value> = screenplay
        .scenes()
        .iter()
        .map(|scene| scene_json(&screenplay, scene))
        .collect();
    let first_scene_with_tag: Vec<Value> = (0..4)
        .map(|tag| scene_index(&screenplay, screenplay.find_first_scene_with_tag(tag)))
        .collect();
    let time_for_tag: Vec<Value> = (0..4_u8)
        .map(|tag| {
            let time_msec: Vec<u32> = WARPED_TIMES_SEC
                .iter()
                .map(|&t| {
                    screenplay
                        .time_msec_for_scene_tag_and_warped_time_in_scene(tag, t)
                        .unwrap_or(u32::MAX)
                })
                .collect();
            let time_sec: Vec<f32> = WARPED_TIMES_SEC
                .iter()
                .map(|&t| screenplay.time_sec_for_scene_tag_and_warped_time_in_scene(tag, t))
                .collect();
            json!({ "tag": tag, "time_ms": time_msec, "time_sec": time_sec })
        })
        .collect();

    let mut flag_operations = Vec::new();
    foreign.set_flags(
        ScreenplaySceneFlags::from_bits_retain(0x0f),
        ScreenplaySceneFlags::from_bits_retain(0x05),
    );
    flag_operations.push(foreign.flags().bits());
    foreign.set_flags(
        ScreenplaySceneFlags::from_bits_retain(0x03),
        ScreenplaySceneFlags::from_bits_retain(0x02),
    );
    flag_operations.push(foreign.flags().bits());
    foreign.clear_flag(ScreenplaySceneFlags::from_bits_retain(0x04));
    flag_operations.push(foreign.flags().bits());
    foreign.set_flag(ScreenplaySceneFlags::from_bits_retain(0x80));
    flag_operations.push(foreign.flags().bits());

    let set_duration_sec: Vec<Value> = DURATIONS_SEC
        .iter()
        .map(|&duration_sec| {
            foreign.set_duration_msec(1234);
            let result = foreign.set_duration_sec(duration_sec);
            json!({
                "duration_sec": duration_sec,
                "result": result_name(&result),
                "resulting_duration_ms": duration_json(foreign),
            })
        })
        .collect();
    let contains_foreign_scene = screenplay.contains_scene(other.scenes().first().expect("scene"));

    let loads: Vec<Value> = golden["update_from_binary_file_in_memory"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(load)
        .collect();

    Replay::Present(json!({
        "scenes": scenes,
        "contains_foreign_scene": contains_foreign_scene,
        "first_scene_with_tag": first_scene_with_tag,
        "warped_times_sec": WARPED_TIMES_SEC,
        "time_for_tag_and_warped_time": time_for_tag,
        "flag_operations": flag_operations,
        "set_duration_sec": set_duration_sec,
        "update_from_binary_file_in_memory": loads,
    }))
}

/// The screenplay of the generator: three scenes, the first and the last of
/// which have the same tag.
fn screenplay() -> Result<Screenplay<'static>, Box<dyn std::error::Error>> {
    let p = |x, y, z| Vector3WithYaw { x, y, z, yaw: 0.0 };
    let mut screenplay = Screenplay::new();

    // Tag 1, dynamic duration, trajectory longer than the scene
    let scene = screenplay.append_new_scene()?;
    scene.set_duration_msec(5000);
    scene.set_tag(1);
    scene.set_flag(ScreenplaySceneFlags::DYNAMIC_DURATION);
    scene.set_trajectory(Some(Rc::new(trajectory(
        p(0.0, 0.0, 0.0),
        p(0.0, 0.0, 3000.0),
        6000,
    )?)));
    scene
        .time_axis_mut()
        .append_segment(TimeSegment::realtime(5000))?;

    // Tag 2, starts at 5 s, double speed then real time
    let scene = screenplay.append_new_scene()?;
    scene.set_duration_msec(4000);
    scene.set_origin_msec(5000)?;
    scene.set_tag(2);
    scene.set_trajectory(Some(Rc::new(trajectory(
        p(0.0, 0.0, 3000.0),
        p(3000.0, 0.0, 3000.0),
        8000,
    )?)));
    scene
        .time_axis_mut()
        .append_segment(TimeSegment::constant_rate(2000, 2.0))?;
    scene
        .time_axis_mut()
        .append_segment(TimeSegment::realtime(2000))?;

    // Tag 1 again, starts at 9 s, infinite, no segments
    let scene = screenplay.append_new_scene()?;
    scene.set_infinite();
    scene.set_origin_msec(9000)?;
    scene.set_tag(1);
    scene.set_trajectory(Some(Rc::new(trajectory(
        p(3000.0, 0.0, 3000.0),
        p(3000.0, 0.0, 0.0),
        3000,
    )?)));

    Ok(screenplay)
}

/// A trajectory that moves from the start to the end in the given time, as
/// `build_trajectory()` of the generator.
fn trajectory(
    start: Vector3WithYaw,
    end: Vector3WithYaw,
    duration_msec: u32,
) -> Result<Trajectory<'static>, Box<dyn std::error::Error>> {
    let mut builder = TrajectoryBuilder::new(1)?;
    builder.set_start_position(start)?;
    builder.move_to_in_time(end, duration_msec, 2000.0)?;
    Ok(builder.build()?)
}

fn scene_json(screenplay: &Screenplay<'_>, scene: &ScreenplayScene<'_>) -> Value {
    let masks = || (0..4).map(ScreenplaySceneFlags::from_bits_retain);
    let has_any_flag: Vec<bool> = masks().map(|mask| scene.has_any_flag(mask)).collect();
    let has_all_flags: Vec<bool> = masks().map(|mask| scene.has_all_flags(mask)).collect();
    let uncovered = scene.uncovered_trajectory_duration_sec();
    json!({
        "tag": scene.tag(),
        "flags": scene.flags().bits(),
        "origin_ms": scene.origin_msec(),
        "duration_ms": duration_json(scene),
        "is_infinite": scene.is_infinite(),
        "has_any_flag": has_any_flag,
        "has_all_flags": has_all_flags,
        "uncovered_trajectory_duration_result": if uncovered.is_some() { "SUCCESS" } else { "EINVAL" },
        "uncovered_trajectory_duration_sec": uncovered.unwrap_or(f32::NAN),
        "warped_time_remaining_from_trajectory_sec":
            scene.warped_time_remaining_from_trajectory_at_end_of_time_axis(),
        "contained": screenplay.contains_scene(scene),
    })
}

fn scene_index(screenplay: &Screenplay<'_>, scene: Option<&ScreenplayScene<'_>>) -> Value {
    json!(scene.and_then(|scene| {
        screenplay
            .scenes()
            .iter()
            .position(|other| ptr::addr_eq(other, scene))
    }))
}

/// An infinite duration is recorded as `null`.
fn duration_json(scene: &ScreenplayScene<'_>) -> Value {
    if scene.is_infinite() {
        Value::Null
    } else {
        json!(scene.duration_msec())
    }
}

/// Loads a screenplay from a show file of the golden data. The C library
/// gives an empty screenplay for an empty buffer.
fn load(golden: &Value) -> Value {
    let show = decode_hex(golden["show_hex"].as_str().expect("show"));
    let screenplay = if show.is_empty() {
        Ok(Screenplay::new())
    } else {
        BinaryFile::new(&show)
            .map_err(|err| err.error_name())
            .and_then(|file| Screenplay::from_binary_file(&file).map_err(|err| err.error_name()))
    };
    let mut result = Map::new();
    result.insert("input".into(), golden["input"].clone());
    result.insert("show_hex".into(), json!(encode_hex(&show)));
    match screenplay {
        Ok(screenplay) => {
            result.insert("result".into(), json!("SUCCESS"));
            result.insert("num_scenes".into(), json!(screenplay.len()));
            if let Some(scene) = screenplay.scene(0) {
                result.insert("duration_ms".into(), duration_json(scene));
            }
        }
        Err(name) => {
            result.insert("result".into(), json!(name));
            result.insert("num_scenes".into(), json!(0));
        }
    }
    Value::Object(result)
}
