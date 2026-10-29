//! Replays the `screenplays` section of `scenarios.json`: screenplays of
//! several scenes played by the show controller.

use std::rc::Rc;

use serde_json::{Map, Value, json};
use skybrush::control::{ControlComponents, ShowController};
use skybrush::lights::LightProgram;
use skybrush::screenplay::Screenplay;
use skybrush::time_axis::TimeSegment;
use skybrush::trajectory::Trajectory;
use skybrush::yaw_control::YawControl;

use crate::common::Replay;
use crate::common::control::output;
use crate::common::errors::result_name;
use crate::common::oracle::decode_hex;
use crate::common::values::{f32_of, u32_of};

pub fn replay_screenplays(golden: &Value) -> Replay {
    let scenarios = golden.as_array().expect("scenarios");
    Replay::Present(scenarios.iter().map(replay_screenplay).collect())
}

fn replay_screenplay(golden: &Value) -> Value {
    let mut screenplay = Screenplay::new();
    let scenes: Vec<Value> = golden["scenes"]
        .as_array()
        .expect("scenes")
        .iter()
        .map(|spec| add_scene(&mut screenplay, spec))
        .collect();
    let mut ctrl = ShowController::new(screenplay);
    let samples: Vec<Value> = golden["samples"]
        .as_array()
        .expect("samples")
        .iter()
        .map(|sample| replay_sample(&mut ctrl, sample))
        .collect();
    json!({ "scenario": golden["scenario"], "scenes": scenes, "samples": samples })
}

/// Appends a scene with the given specification to the screenplay.
fn add_scene(screenplay: &mut Screenplay<'static>, spec: &Value) -> Value {
    let scene = screenplay.append_new_scene().expect("memory");
    let mut result = Map::new();
    if let Some(duration) = spec["duration_ms"].as_u64() {
        scene.set_duration_msec(u32::try_from(duration).expect("a duration"));
        result.insert("duration_ms".into(), json!(duration));
    } else {
        scene.set_infinite();
        result.insert("duration_ms".into(), Value::Null);
    }
    let origin_msec = i32::try_from(spec["origin_ms"].as_i64().expect("origin")).expect("i32");
    result.insert("origin_ms".into(), json!(origin_msec));
    result.insert(
        "set_origin_result".into(),
        json!(result_name(&scene.set_origin_msec(origin_msec))),
    );

    let hex = |key: &str| decode_hex(spec[key].as_str().expect("hex"));
    let trajectory = Trajectory::from_bytes(hex("trajectory_hex")).expect("trajectory");
    scene.set_trajectory(Some(Rc::new(trajectory)));
    result.insert("trajectory_hex".into(), spec["trajectory_hex"].clone());
    scene.set_light_program(Some(Rc::new(LightProgram::from_bytes(hex(
        "light_program_hex",
    )))));
    result.insert(
        "light_program_hex".into(),
        spec["light_program_hex"].clone(),
    );

    if spec["yaw_deg"].is_null() {
        result.insert("yaw_deg".into(), Value::Null);
    } else {
        let yaw_deg = f32_of(&spec["yaw_deg"]);
        let mut yaw = YawControl::new();
        yaw.set_constant_yaw(yaw_deg).expect("memory");
        scene.set_yaw_control(Some(Rc::new(yaw)));
        result.insert("yaw_deg".into(), json!(yaw_deg));
    }

    let segments: Vec<Value> = spec["time_axis"]
        .as_array()
        .expect("time axis")
        .iter()
        .map(|segment| {
            let duration_msec = u32_of(&segment["duration_ms"]);
            let initial_rate = f32_of(&segment["initial_rate"]);
            let final_rate = f32_of(&segment["final_rate"]);
            scene
                .time_axis_mut()
                .append_segment(TimeSegment::new(duration_msec, initial_rate, final_rate))
                .expect("valid segment");
            json!({
                "duration_ms": duration_msec,
                "initial_rate": initial_rate,
                "final_rate": final_rate,
            })
        })
        .collect();
    result.insert("time_axis".into(), segments.into());
    Value::Object(result)
}

/// Updates the controller to the time of a sample, and records its output.
fn replay_sample(ctrl: &mut ShowController<'_>, sample: &Value) -> Value {
    let time_msec = u32_of(&sample["time_ms"]);
    ctrl.update_time_msec(time_msec);
    let scene = ctrl
        .screenplay()
        .scene_at_time_msec(time_msec)
        .map_or(-1, |(index, _)| i64::try_from(index).expect("an index"));
    let output_time = ctrl.current_output_time();
    let mut result = output(ctrl.current_output());
    result.insert("time_ms".into(), json!(time_msec));
    result.insert("scene".into(), json!(scene));
    result.insert(
        "components".into(),
        json!(components(ctrl.current_output().components())),
    );
    result.insert("output_time_ms".into(), json!(output_time.time_msec));
    result.insert(
        "time_in_scene_ms".into(),
        json!(output_time.time_in_scene_msec),
    );
    result.insert(
        "warped_time_in_scene_sec".into(),
        json!(output_time.warped_time_in_scene_sec),
    );
    result.insert("ended".into(), json!(ctrl.has_reached_end()));
    Value::Object(result)
}

/// The known components of an output, as a mask.
fn components(components: ControlComponents) -> u8 {
    (components & ControlComponents::ALL).bits()
}
