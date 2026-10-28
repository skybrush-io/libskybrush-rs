//! Replays the `colors` section of `scenarios.json`: the public color helpers.

use serde_json::{Map, Value, json};
use skybrush::colors::{RgbColor, RgbwColor, RgbwConversion};

use crate::common::Replay;
use crate::common::values::to_f32;

pub fn replay(golden: &Value) -> Replay {
    let golden = golden.as_object().expect("colors");
    let mut actual = Map::new();
    for (key, cases) in golden {
        let cases = cases.as_array().expect("cases of a color helper");
        let replay_case: fn(&Value) -> Value = match key.as_str() {
            "from_color_temperature" => from_color_temperature,
            "rgb565" => rgb565,
            "linear_interpolation" => linear_interpolation,
            "rgbw" => rgbw,
            // Left out, so that the comparison reports it as missing
            _ => continue,
        };
        actual.insert(key.clone(), cases.iter().map(replay_case).collect());
    }
    Replay::Present(Value::Object(actual))
}

fn from_color_temperature(case: &Value) -> Value {
    let temperature = &case["temperature_k"];
    let color = RgbColor::from_color_temperature(number_to_f32(temperature));
    json!({ "temperature_k": temperature, "color_approx8": rgb_to_json(color) })
}

fn rgb565(case: &Value) -> Value {
    let color = rgb_from_json(&case["color"]);
    let encoded = color.encode_rgb565();
    json!({
        "color": rgb_to_json(color),
        "encoded": encoded,
        "decoded": rgb_to_json(RgbColor::decode_rgb565(encoded)),
    })
}

fn linear_interpolation(case: &Value) -> Value {
    let first = rgb_from_json(&case["first"]);
    let second = rgb_from_json(&case["second"]);
    let ratio = &case["ratio"];
    let result = first.linear_interpolation(second, number_to_f32(ratio));
    json!({
        "first": rgb_to_json(first),
        "second": rgb_to_json(second),
        "ratio": ratio,
        "result": rgb_to_json(result),
    })
}

fn rgbw(case: &Value) -> Value {
    let name = case["conversion"].as_str().expect("name of the conversion");
    let conversion = conversion_from_name(name);
    // Conversions based on a color temperature use powf() and logf(), whose
    // results may differ by one between math libraries.
    let key = if conversion.temperature().is_some() {
        "rgbw_approx8"
    } else {
        "rgbw"
    };
    let results: Vec<Value> = case["results"]
        .as_array()
        .expect("results of the conversion")
        .iter()
        .map(|result| {
            let color = rgb_from_json(&result["color"]);
            json!({ "color": rgb_to_json(color), key: rgbw_to_json(color.to_rgbw(&conversion)) })
        })
        .collect();
    json!({ "conversion": name, "results": results })
}

/// Sets up a conversion from its name in the trace, e.g. `fixed_value_128`
/// or `reference_color_255_200_100`.
fn conversion_from_name(name: &str) -> RgbwConversion {
    let number = |text: &str| -> u32 {
        text.parse()
            .unwrap_or_else(|_| panic!("bad number in conversion {name}"))
    };
    let byte = |text: &str| u8::try_from(number(text)).expect("color component");
    if name == "off" {
        RgbwConversion::off()
    } else if name == "min_subtraction" {
        RgbwConversion::min_subtraction()
    } else if let Some(value) = name.strip_prefix("fixed_value_") {
        RgbwConversion::fixed_value(byte(value))
    } else if let Some(temperature) = name.strip_prefix("color_temperature_") {
        RgbwConversion::color_temperature(to_f32(f64::from(number(temperature))))
    } else if let Some(components) = name.strip_prefix("reference_color_") {
        let components: Vec<u8> = components.split('_').map(byte).collect();
        let [red, green, blue] = components[..] else {
            panic!("bad reference color in conversion {name}")
        };
        RgbwConversion::reference_color(RgbColor::new(red, green, blue))
    } else {
        panic!("unknown conversion {name}")
    }
}

fn rgb_from_json(value: &Value) -> RgbColor {
    let components: Vec<u8> = value
        .as_array()
        .expect("color")
        .iter()
        .map(|component| {
            let component = component.as_u64().expect("color component");
            u8::try_from(component).expect("color component")
        })
        .collect();
    let [red, green, blue] = components[..] else {
        panic!("bad color {value}")
    };
    RgbColor::new(red, green, blue)
}

fn rgb_to_json(color: RgbColor) -> Value {
    json!([color.red, color.green, color.blue])
}

fn rgbw_to_json(color: RgbwColor) -> Value {
    json!([color.red, color.green, color.blue, color.white])
}

/// Converts a number of the trace to `f32`. Floats in the traces are `f32`
/// values printed with enough digits to read them back exactly.
fn number_to_f32(value: &Value) -> f32 {
    to_f32(value.as_f64().expect("number"))
}
