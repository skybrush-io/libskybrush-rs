//! Replays the `show_controller` section of a format trace: the output of
//! the show controller at the sample times of the trace, and the events that
//! it fires.

use serde_json::{Map, Value, json};
use skybrush::control::ShowController;
use skybrush::formats::binary::BinaryFile;
use skybrush::screenplay::Screenplay;

use crate::common::Replay;
use crate::common::control::output;
use crate::common::events::event;
use crate::common::values::sample_times;

pub fn replay(file: &BinaryFile<'_>, golden: &Map<String, Value>) -> Replay {
    let screenplay = Screenplay::from_binary_file(file).expect("valid screenplay");
    let mut ctrl = ShowController::new(screenplay);
    let samples: Vec<Value> = sample_times(golden)
        .map(|(msec, _)| {
            ctrl.update_time_msec(msec);
            let mut sample = output(ctrl.current_output());
            sample.insert("ended".into(), json!(ctrl.has_reached_end()));
            Value::Object(sample)
        })
        .collect();

    // A new controller fires the events in order while the time moves
    // forward
    let mut ctrl = ShowController::new(ctrl.into_screenplay());
    let mut fired = Vec::new();
    for (msec, _) in sample_times(golden) {
        ctrl.update_time_msec(msec);
        while let Some(fired_event) = ctrl.next_event() {
            fired.push(json!([msec, event(fired_event)]));
        }
    }
    Replay::Present(json!({ "samples": samples, "fired_events": fired }))
}
