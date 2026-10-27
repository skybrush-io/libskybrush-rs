//! Comparison of a trace with a golden trace.
//!
//! This is a port of `TraceComparator` in `test/traces/compare_traces.py` of
//! libskybrush, which is the reference implementation of the comparison. It
//! must report a difference exactly where the reference does.

use serde_json::{Map, Number, Value};

/// Keys that are not compared because they do not describe the behaviour.
const IGNORED_KEYS: &[&str] = &["generator"];

/// Absolute and relative tolerance of a quantity.
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    pub abs: f64,
    pub rel: f64,
}

impl Tolerance {
    /// A tolerance that allows no difference at all.
    pub const EXACT: Self = Self { abs: 0.0, rel: 0.0 };

    fn from_json(value: &Value) -> Self {
        let get = |name: &str| {
            value
                .get(name)
                .and_then(Value::as_f64)
                .unwrap_or_else(|| panic!("tolerance without {name}: {value}"))
        };
        Self {
            abs: get("abs"),
            rel: get("rel"),
        }
    }
}

/// Tolerances of the units, determined by the suffix of a key.
#[derive(Clone, Debug)]
pub struct Tolerances {
    /// Units and their tolerances, longest unit first so that `mm_s2` is
    /// matched before `mm_s`.
    by_unit: Vec<(String, Tolerance)>,
    default: Tolerance,
}

impl Tolerances {
    pub fn new(by_unit: impl IntoIterator<Item = (String, Tolerance)>, default: Tolerance) -> Self {
        let mut by_unit: Vec<_> = by_unit.into_iter().collect();
        by_unit.sort_by_key(|(unit, _)| core::cmp::Reverse(unit.len()));
        Self { by_unit, default }
    }

    /// Reads the tolerances from the contents of a `tolerances.json` file.
    pub fn from_json(value: &Value) -> Self {
        assert_eq!(value["version"], 1, "unsupported version of the tolerances");
        let by_unit = value["by_unit"]
            .as_object()
            .expect("tolerances without by_unit")
            .iter()
            .map(|(unit, tolerance)| (unit.clone(), Tolerance::from_json(tolerance)));
        Self::new(by_unit, Tolerance::from_json(&value["default"]))
    }

    fn of_unit(&self, key: Option<&str>) -> Option<Tolerance> {
        let key = key?;
        self.by_unit.iter().find_map(|(unit, tolerance)| {
            let prefix = key.strip_suffix(unit.as_str())?;
            prefix.ends_with('_').then_some(*tolerance)
        })
    }
}

/// Compares values with golden values and collects the differences.
pub struct Comparator<'a> {
    tolerances: &'a Tolerances,
    differences: Vec<String>,
}

impl<'a> Comparator<'a> {
    pub fn new(tolerances: &'a Tolerances) -> Self {
        Self {
            tolerances,
            differences: Vec::new(),
        }
    }

    /// The differences found so far, as `<path>: <message>`.
    pub fn into_differences(self) -> Vec<String> {
        self.differences
    }

    /// Compares a whole trace with a golden trace.
    pub fn compare_root(&mut self, expected: &Value, actual: &Value) {
        self.compare(expected, actual, "", None, None);
    }

    /// Compares the member `key` of the golden object `parent` with a value.
    ///
    /// This finds the same differences as comparing the whole objects would
    /// find within that member, so a trace can be compared section by section.
    pub fn compare_member(&mut self, parent: &Map<String, Value>, key: &str, actual: &Value) {
        let expected = parent
            .get(key)
            .unwrap_or_else(|| panic!("the golden object has no member {key}"));
        self.compare(
            expected,
            actual,
            &format!(".{key}"),
            Some(key),
            Some(parent),
        );
    }

    fn compare(
        &mut self,
        expected: &Value,
        actual: &Value,
        path: &str,
        key: Option<&str>,
        context: Option<&Map<String, Value>>,
    ) {
        match (expected, actual) {
            (Value::Object(expected), Value::Object(actual)) => {
                self.compare_objects(expected, actual, path);
            }
            (Value::Array(expected), Value::Array(actual)) => {
                self.compare_arrays(expected, actual, path, key, context);
            }
            (Value::Number(expected), Value::Number(actual)) => {
                self.compare_numbers(expected, actual, path, key);
            }
            // Booleans and nulls end up here too, so they must match exactly
            _ => {
                if expected != actual {
                    self.report(path, &format!("expected {expected}, got {actual}"));
                }
            }
        }
    }

    fn compare_objects(
        &mut self,
        expected: &Map<String, Value>,
        actual: &Map<String, Value>,
        path: &str,
    ) {
        let compared = |key: &&String| !IGNORED_KEYS.contains(&key.as_str());
        // The maps are sorted by key, like the sorted() calls of the reference.
        for key in expected.keys().filter(compared) {
            if !actual.contains_key(key) {
                self.report(&format!("{path}.{key}"), "missing from the trace");
            }
        }
        for key in actual.keys().filter(compared) {
            if !expected.contains_key(key) {
                self.report(&format!("{path}.{key}"), "not present in the golden trace");
            }
        }
        for (key, expected_value) in expected.iter().filter(|(key, _)| compared(key)) {
            if let Some(actual_value) = actual.get(key) {
                let path = format!("{path}.{key}");
                self.compare(
                    expected_value,
                    actual_value,
                    &path,
                    Some(key),
                    Some(expected),
                );
            }
        }
    }

    fn compare_arrays(
        &mut self,
        expected: &[Value],
        actual: &[Value],
        path: &str,
        key: Option<&str>,
        context: Option<&Map<String, Value>>,
    ) {
        if expected.len() != actual.len() {
            let message = format!("expected {} items, got {}", expected.len(), actual.len());
            self.report(path, &message);
            return;
        }

        // Columns of RTH plan entries are named in the "entry_fields" array
        let columns = match (key, context.and_then(|context| context.get("entry_fields"))) {
            (Some("entries"), Some(Value::Array(columns))) => Some(columns),
            _ => None,
        };

        for (index, (expected_item, actual_item)) in expected.iter().zip(actual).enumerate() {
            let item_path = format!("{path}[{index}]");
            match (columns, expected_item) {
                (Some(columns), Value::Array(expected_fields)) => {
                    let actual_fields = actual_item.as_array().map_or(&[][..], Vec::as_slice);
                    for (column, (e, a)) in expected_fields.iter().zip(actual_fields).enumerate() {
                        let name = columns
                            .get(column)
                            .and_then(Value::as_str)
                            .unwrap_or_else(|| panic!("{item_path}: unnamed column {column}"));
                        self.compare(e, a, &format!("{item_path}.{name}"), Some(name), None);
                    }
                    if expected_fields.len() != actual_fields.len() || !actual_item.is_array() {
                        let message = format!(
                            "expected {} fields, got {}",
                            expected_fields.len(),
                            actual_fields.len()
                        );
                        self.report(&item_path, &message);
                    }
                }
                // Items of an array inherit the key (and thus the unit) of the array
                _ => self.compare(expected_item, actual_item, &item_path, key, context),
            }
        }
    }

    fn compare_numbers(
        &mut self,
        expected: &Number,
        actual: &Number,
        path: &str,
        key: Option<&str>,
    ) {
        let unit = self.tolerances.of_unit(key);
        if unit.is_none()
            && let (Some(e), Some(a)) = (as_integer(expected), as_integer(actual))
        {
            if e != a {
                self.report(path, &format!("expected {e}, got {a}"));
            }
            return;
        }

        let tolerance = unit.unwrap_or(self.tolerances.default);
        let (e, a) = (to_f64(expected), to_f64(actual));
        let allowed = tolerance.abs + tolerance.rel * e.abs();
        // math.isclose(e, a, rel_tol=0, abs_tol=allowed); JSON has no infinities
        if (e - a).abs() > allowed {
            let message = format!("expected {e:?}, got {a:?} (allowed difference: {allowed})");
            self.report(path, &message);
        }
    }

    fn report(&mut self, path: &str, message: &str) {
        let path = if path.is_empty() { "." } else { path };
        self.differences.push(format!("{path}: {message}"));
    }
}

/// The value of a number if it was written as an integer in the JSON source.
fn as_integer(number: &Number) -> Option<i128> {
    number
        .as_u64()
        .map(i128::from)
        .or_else(|| number.as_i64().map(i128::from))
}

fn to_f64(number: &Number) -> f64 {
    number
        .as_f64()
        .expect("a JSON number is representable as f64")
}
